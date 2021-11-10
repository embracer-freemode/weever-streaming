use anyhow::Result;


/// log out error messages if we have error with anyhow type
pub async fn catch<F>(future: F)
where
	F: std::future::Future<Output = Result<()>>,
{
	if let Err(err) = future.await {
		log::error!("{:?}", err);
	}
}
