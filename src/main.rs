mod kubernetes;
mod route53;

use aws_sdk_route53::{
    types::{Change, ResourceRecordSet},
    Client as Route53Client,
};
use futures::TryStreamExt;
use k8s_openapi::api::core::v1::Service;
use kube::{runtime::watcher::Event, Client as KubeClient};
use kubernetes::map_to_records;
use log::{debug, error, info};
use route53::{apply_changes, list_records};
use tokio::{
    select,
    signal::unix::{signal, SignalKind},
};

fn calculate_reconciliation_step(
    event: Event<Service>,
    existing_records: &[ResourceRecordSet],
) -> Vec<Change> {
    let mut changes = Vec::new();

    match event {
        Event::Applied(service) => {
            for record in map_to_records(service).unwrap_or_default() {
                if let Some(change) = record.reconcile_with(existing_records) {
                    changes.push(change);
                }
            }
        }
        Event::Deleted(service) => {
            for record in map_to_records(service).unwrap_or_default() {
                changes.push(record.as_delete());
            }
        }
        Event::Restarted(_) => {}
    }

    changes
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

    // Produce the watcher for listening to future events
    let mut watcher = Box::pin(kubernetes::watch(&kube_client));

    // Catch up to existing services
    debug!("starting initial service record sync");
    let existing_records = list_records(&route53_client).await;
    let changes = kubernetes::get_all(&kube_client)
        .await
        .into_iter()
        .map(Event::Applied)
        .flat_map(|event| calculate_reconciliation_step(event, &existing_records))
        .collect();

    apply_changes(&route53_client, changes).await;

    debug!("initial service sync complete, starting reconciliation loop");
    loop {
        match select! {
            event = watcher.try_next() => Some(event),
            _ = quit.recv() => None,
            _ = terminate.recv() => None,
            _ = interrupt.recv() => None,
        } {
            Some(Ok(Some(event))) => {
                let existing_records = list_records(&route53_client).await;

                let changes = calculate_reconciliation_step(event, &existing_records);

                apply_changes(&route53_client, changes).await;
            }
            Some(Ok(None)) => {
                info!("service event watcher stream ended, shutting down.");
                break;
            }
            Some(Err(err)) => {
                error!("service event watcher stream error: {}", err);
                break;
            }
            None => {
                info!("received termination, quit or interrupt signal, shutting down.");
                break;
            }
        }
    }
}
