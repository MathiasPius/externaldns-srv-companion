mod discovery;
mod route53;

use aws_sdk_route53::{model::ResourceRecord, Client as Route53Client};
use discovery::discover_services;
use kube::Client as KubeClient;
use log::debug;
use route53::list_records;

#[tokio::main]
async fn main() {
    pretty_env_logger::init_timed();

    let shared_config = aws_config::load_from_env().await;
    let route53_client = Route53Client::new(&shared_config);

    let kube_client = KubeClient::try_default().await.expect("create kube client");

    let desired_records = discover_services(&kube_client).await;
    let existing_records = list_records(&route53_client).await;

    for desired_record in desired_records {
        debug!(
            "checking if {record_name} already exists in any hosted zones.",
            record_name = desired_record.record_name()
        );

        if let Some(existing_record) = existing_records
            .iter()
            .find(|existing| existing.name == Some(desired_record.record_name()))
        {
            if let Some(values) = existing_record.resource_records() {
                if values
                    .iter()
                    .filter_map(ResourceRecord::value)
                    .any(|value| value == desired_record.record_value())
                {
                    debug!("no value set necessary, already defined");
                } else {
                    println!("update: {:?}", existing_record.name().unwrap());

                    
                }
            }
        } else {
            println!("create: {:?}", desired_record.record_name());
        }
    }
}
