use aws_sdk_route53::{
    error::ListHostedZonesError, model::ResourceRecordSet, types::SdkError, Client,
};
use log::{debug, trace};

pub async fn get_hosted_zone_by_hostname(
    client: &Client,
    hostname: &str,
) -> Result<Option<String>, SdkError<ListHostedZonesError>> {
    let hosted_zones = client.list_hosted_zones().send().await?;
    for hosted_zone in hosted_zones.hosted_zones().unwrap_or_default() {
        if let Some(name) = hosted_zone.name() {
            if hostname.ends_with(name) {
                return Ok(hosted_zone.id.clone());
            }
        }
    }

    Ok(None)
}

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
