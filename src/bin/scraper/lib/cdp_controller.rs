// use chromiumoxide::browser::Browser;

// pub struct CdpController {
//     browser: Browser,
// }

// impl CdpController {
//     pub async fn new() -> Self {
//         let (browser, _) = Browser::launch(BrowserConfig::default()).await.unwrap();
//         Self { browser }
//     }

//     pub async fn navigate(&self, url: &str) -> Result<(), Box<dyn std::error::Error>> {
//         let page = self.browser.new_page(url).await?;
//         page.wait_for_navigation().await
//     }
// }
