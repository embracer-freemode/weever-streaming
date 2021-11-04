use anyhow::Result;


pub async fn catch<F>(future: F)
where
	F: std::future::Future<Output = Result<()>>,
{
	if let Err(err) = future.await {
		log::error!("{:?}", err);
	}
}
