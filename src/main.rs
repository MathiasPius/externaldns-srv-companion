mod discovery;
mod route53;

use std::time::Duration;

use aws_sdk_route53::Client as Route53Client;
use discovery::discover_services;
use kube::Client as KubeClient;
use log::debug;
use route53::{apply_changes, list_records};
use tokio::{
    select,
    signal::{
        self,
        unix::{signal, SignalKind},
    },
};

async fn reconciliation_loop(kube_client: &KubeClient, route53_client: &Route53Client) {
    let desired_records = discover_services(kube_client).await;
    let existing_records = list_records(route53_client).await;

    let changes = desired_records
        .into_iter()
        .filter_map(|desired_record| {
            debug!(
                "checking if {record_name} already exists in any hosted zones.",
                record_name = desired_record.record_name()
            );

            desired_record.reconcile_with(&existing_records)
        })
        .collect();

    apply_changes(route53_client, changes).await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    pretty_env_logger::init_timed();

    let shared_config = aws_config::load_from_env().await;
    let route53_client = Route53Client::new(&shared_config);

    let kube_client = KubeClient::try_default().await.expect("create kube client");

    let mut quit = signal(SignalKind::quit()).ok().unwrap();
    let mut terminate = signal(SignalKind::terminate()).ok().unwrap();
    let mut interrupt = signal(SignalKind::interrupt()).ok().unwrap();

    loop {
        reconciliation_loop(&kube_client, &route53_client).await;

        if select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => false,
            _ = quit.recv() => true,
            _ = terminate.recv() => true,
            _ = interrupt.recv() => true,
        } {
            break;
        }
    }
}
