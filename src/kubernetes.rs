use aws_sdk_route53::model::{Change, ChangeAction, ResourceRecord, ResourceRecordSet, RrType};
use futures::Stream;
use k8s_openapi::api::core::v1::Service;
use kube::{
    api::ListParams,
    runtime::watcher::{watcher, Error, Event},
    Api, Client, Resource,
};
use log::debug;

#[derive(Debug)]
pub struct ServiceRecord {
    hostname: String,
    name: String,
    protocol: String,
    port: i32,
}

impl From<&ServiceRecord> for ResourceRecordSet {
    fn from(record: &ServiceRecord) -> Self {
        ResourceRecordSet::builder()
            .set_type(Some(RrType::Srv))
            .set_name(Some(record.record_name()))
            .set_ttl(Some(1800))
            .set_resource_records(Some(vec![ResourceRecord::builder()
                .set_value(Some(record.record_value()))
                .build()]))
            .build()
    }
}

impl ServiceRecord {
    pub fn record_name(&self) -> String {
        format!(
            "_{name}._{protocol}.{hostname}",
            name = self.name,
            protocol = self.protocol.to_lowercase(),
            hostname = self.hostname
        )
    }

    pub fn record_value(&self) -> String {
        format!(
            "0 10 {port} {hostname}",
            port = self.port,
            hostname = self.hostname
        )
    }

    /// Express the ServiceRecord as an Upsert change
    pub fn as_upsert(&self) -> Change {
        Change::builder()
            .action(ChangeAction::Upsert)
            .resource_record_set(self.into())
            .build()
    }

    /// Express the ServiceRecord as a Create change
    pub fn as_create(&self) -> Change {
        Change::builder()
            .action(ChangeAction::Upsert)
            .resource_record_set(self.into())
            .build()
    }

    pub fn as_delete(&self) -> Change {
        Change::builder()
            .action(ChangeAction::Delete)
            .resource_record_set(self.into())
            .build()
    }

    pub fn reconcile_with(&self, existing_records: &[ResourceRecordSet]) -> Option<Change> {
        let existing_record = existing_records
            .iter()
            .find(|existing| existing.name == Some(self.record_name()));

        if let Some(existing_record) = existing_record {
            if let Some(values) = existing_record.resource_records() {
                if values
                    .iter()
                    .filter_map(ResourceRecord::value)
                    .any(|value| value == self.record_value())
                {
                    debug!(
                        "record {} already matches desired state {}",
                        self.record_name(),
                        self.record_value()
                    );
                    None
                } else {
                    debug!(
                        "record {} does not match desired state, upsert. desired: \"{}\", actual: {:?}",
                        self.record_name(),
                        self.record_value(),
                        existing_record.resource_records()
                    );

                    Some(self.as_upsert())
                }
            } else {
                debug!(
                    "record {} does not define any resource records, upsert. desired: \"{}\"",
                    self.record_name(),
                    self.record_value()
                );
                Some(self.as_upsert())
            }
        } else {
            debug!(
                "record {} not found in any hosted zones, creating as: \"{}\"",
                self.record_name(),
                self.record_value()
            );
            Some(self.as_create())
        }
    }
}

pub fn map_to_records(service: Service) -> Option<Vec<ServiceRecord>> {
    let annotations = service.meta().annotations.as_ref()?;
    let spec = service.spec.as_ref()?;

    // I really don't think it's all that likely that a service is missing
    // namespace and name, but might as well cover our bases.
    let service_fqdn = format!(
        "{}/{}",
        service
            .meta()
            .namespace
            .as_deref()
            .unwrap_or("<NO NAMESPACE>"),
        service.meta().name.as_deref().unwrap_or("<NO NAME>")
    );

    if spec.type_.as_ref()? != "NodePort" {
        debug!("Service {service_fqdn} is not a NodePort service, ignoring.");
        return None;
    }

    // The ExternalDNS annotation is used for defining the hostname used in the service record.
    // This is the part that tails the SRV-specific parts _portname._proto.{hostname}
    let hostname = if let Some(hostname) =
        annotations.get("external-dns.alpha.kubernetes.io/hostname")
    {
        hostname
    } else {
        debug!("NodePort service {service_fqdn} does not have an 'external-dns.alpha.kubernetes.io/hostname' annotation, ignoring.");
        return None;
    };

    // Map all the services' well-formed ports to the ServiceRecord type used for creating the
    // records in AWS Route53 later.
    let srvs = spec
        .ports
        .as_ref()?
        .iter()
        .enumerate()
        .filter_map(|(port_index, port)| {
            let name = if let Some(name) = port.name.as_ref() {
                name.clone()
            } else {
                debug!("NodePort service {service_fqdn}'s port {port_index} did not have a name, ignoring it.");
                return None;
            };

            let protocol = if let Some(protocol) = port.protocol.as_ref() {
                protocol.clone()
            } else {
                debug!("NodePort service {service_fqdn}'s port {port_index} did not have a protocol, ignoring it.");
                return None;
            };

            let port = if let Some(port) = port.node_port {
                port
            } else {
                debug!("NodePort service {service_fqdn}'s port {port_index} did not have an external port, ignoring it.");
                return None;
            };

            let record = ServiceRecord {
                hostname: hostname.clone(),
                name,
                protocol,
                port,
            };
            debug!("discovered service record: {:?}", record);

            Some(record)
        })
        .collect();

    Some(srvs)
}

pub fn watch(client: &Client) -> impl Stream<Item = Result<Event<Service>, Error>> {
    let service_filter = ListParams::default();
    let services = Api::<Service>::all(client.clone());

    watcher(services, service_filter)
}

pub async fn get_all(client: &Client) -> Vec<Service> {
    let service_filter = ListParams::default();

    let services = Api::<Service>::all(client.clone());
    let svcs = services.list(&service_filter).await.unwrap();

    svcs.items
}
