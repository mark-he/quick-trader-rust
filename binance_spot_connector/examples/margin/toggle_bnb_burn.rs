use binance_spot_connector::{
    http::Credentials,
    hyper::{BinanceHttpClient, Error},
    margin,
};
use env_logger::Builder;

#[tokio::main]
async fn main() -> Result<(), Error> {
    Builder::from_default_env()
        .filter(None, log::LevelFilter::Info)
        .init();
    let credentials = Credentials::from_hmac("api-key".to_owned(), "api-secret".to_owned());
    let client = BinanceHttpClient::default().credentials(credentials);
    let request = margin::toggle_bnb_burn()
        .spot_bnb_burn(true)
        .interest_bnb_burn(false);
    let data = client.send(request).await?.into_body_str().await?;
    log::info!("{}", data);
    Ok(())
}
