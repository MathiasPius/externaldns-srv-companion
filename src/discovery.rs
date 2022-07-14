use k8s_openapi::api::core::v1::Service;
use kube::{api::ListParams, Api, Client, Resource};
use log::debug;

#[derive(Debug)]
pub struct ServiceRecord {
    hostname: String,
    name: String,
    protocol: String,
    port: i32,
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
}

fn map_to_records(service: Service) -> Option<Vec<ServiceRecord>> {
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
            .unwrap_or_else(|| "<NO NAMESPACE>"),
        service
            .meta()
            .name
            .as_deref()
            .unwrap_or_else(|| "<NO NAME>")
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

pub async fn discover_services(client: &Client) -> Vec<ServiceRecord> {
    let service_filter = ListParams::default();

    let services = Api::<Service>::all(client.clone());
    let svcs = services.list(&service_filter).await.unwrap();

    svcs.items
        .into_iter()
        .flat_map(map_to_records)
        .flatten()
        .collect()
}
