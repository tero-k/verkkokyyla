use std::collections::HashMap;
use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;

use tokio::sync::Semaphore;

const RDNS_CONCURRENCY: usize = 64;

pub async fn resolve_addr(ip: IpAddr) -> Option<String> {
    match tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&ip)).await {
        Ok(Ok(name)) => Some(name),
        Ok(Err(_)) | Err(_) => None,
    }
}

pub async fn resolve_many(ips: &[IpAddr]) -> HashMap<IpAddr, String> {
    resolve_many_with(ips, resolve_addr).await
}

pub async fn resolve_many_with<F, Fut>(ips: &[IpAddr], lookup: F) -> HashMap<IpAddr, String>
where
    F: Fn(IpAddr) -> Fut + Copy + Send + Sync + 'static,
    Fut: Future<Output = Option<String>> + Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(RDNS_CONCURRENCY));
    let mut set = tokio::task::JoinSet::new();
    for ip in ips.iter().copied() {
        let semaphore = Arc::clone(&semaphore);
        set.spawn(async move {
            let permit = semaphore.acquire_owned().await.ok()?;
            let name = lookup(ip).await?;
            drop(permit);
            Some((ip, name))
        });
    }
    let mut names = HashMap::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(Some((ip, name))) = joined {
            names.insert(ip, name);
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;

    use super::resolve_many_with;

    #[tokio::test]
    async fn rdns_resolves_many_through_injected_lookup() {
        let ips = [IpAddr::from([192, 0, 2, 1]), IpAddr::from([192, 0, 2, 2])];

        let names = resolve_many_with(&ips, |ip| async move { Some(format!("ptr-{ip}")) }).await;

        assert_eq!(
            names.get(&ips[0]).map(String::as_str),
            Some("ptr-192.0.2.1")
        );
        assert_eq!(
            names.get(&ips[1]).map(String::as_str),
            Some("ptr-192.0.2.2")
        );
    }
}
