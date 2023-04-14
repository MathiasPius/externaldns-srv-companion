use std::collections::HashMap;

use aws_sdk_route53::{
    types::{Change, ChangeBatch, ResourceRecordSet},
    Client,
};
use log::{debug, error, trace};

pub async fn list_records(client: &Client) -> Vec<ResourceRecordSet> {
    let hosted_zones = client.list_hosted_zones().send().await.unwrap();

    debug!(
        "discovered {} hosted zones",
        hosted_zones.hosted_zones().unwrap_or_default().len()
    );

    let mut all_records = Vec::new();
    for hz in hosted_zones.hosted_zones().unwrap_or_default() {
        let zone_name = hz.name().unwrap_or_default();
        let zone_id = hz.id().unwrap_or_default();

        debug!("iterating over records for {} ({})", zone_name, zone_id);

        let mut records = client
            .list_resource_record_sets()
            .set_hosted_zone_id(hz.id.clone())
            .send()
            .await
            .unwrap();
        loop {
            if let Some(record_sets) = records.resource_record_sets() {
                all_records.extend_from_slice(record_sets);
            }

            if records.is_truncated() {
                trace!(
                    "record set result was truncated, fetching new batch starting with {:?}",
                    records.next_record_name()
                );
                records = client
                    .list_resource_record_sets()
                    .set_hosted_zone_id(hz.id.clone())
                    .set_start_record_name(records.next_record_name)
                    .send()
                    .await
                    .unwrap();
            } else {
                break;
            }
        }
    }

    debug!(
        "found {} records across {} zones",
        all_records.len(),
        hosted_zones.hosted_zones().unwrap_or_default().len()
    );
    all_records
}

pub async fn apply_changes(client: &Client, changes: Vec<Change>) {
    let hosted_zones = client.list_hosted_zones().send().await.unwrap();
    let hosted_zones = hosted_zones.hosted_zones().unwrap_or_default();

    let get_hosted_zone_by_hostname = |hostname: &str| {
        for hosted_zone in hosted_zones {
            if let Some(name) = hosted_zone.name() {
                if hostname.ends_with(name.trim_end_matches('.')) {
                    let id = hosted_zone.id.clone();
                    debug!("looking up hosted zone by hostname ({hostname}) yielded {id:?}");
                    return id;
                }
            }
        }
        debug!("no hosted zone found for hostname ({hostname})");
        None
    };

    let mut batched_changes: HashMap<String, Vec<Change>> = HashMap::new();
    for change in changes {
        let hostname = change.resource_record_set().unwrap().name().unwrap();

        if let Some(hosted_zone) = get_hosted_zone_by_hostname(hostname) {
            batched_changes
                .entry(hosted_zone)
                .or_insert(vec![])
                .push(change);
        } else {
            error!("hosted zone for change {hostname} could not be found.");
        }
    }

    for (hosted_zone, changes) in batched_changes {
        debug!(
            "applying changes for {hosted_zone} containing {} batched changes",
            changes.len()
        );
        client
            .change_resource_record_sets()
            .set_hosted_zone_id(Some(hosted_zone))
            .set_change_batch(Some(
                ChangeBatch::builder().set_changes(Some(changes)).build(),
            ))
            .send()
            .await
            .unwrap();
    }
}
