use base64::Engine;
use reqwest::Client;

use serenity::all::{CreateAttachment, CreateMessage, ReactionType};
use serenity::async_trait;
use serenity::model::channel::Message;
use serenity::prelude::*;

use regex::Regex;

use scraper::{Html, Selector};
use tokio::task::spawn_blocking;
use url::Url;

pub struct Handler;

fn calculate_hash(url: &str, salt: &str) -> String {
    let encoded_url = base64::engine::general_purpose::STANDARD.encode(url);
    let encoded_salt = base64::engine::general_purpose::STANDARD.encode(salt);
    format!("{}L{}L{}", encoded_url, url.len() + 1_000, encoded_salt)
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, data_about_bot: serenity::model::prelude::Ready) {
        println!("{} is connected!", data_about_bot.user.name);
    }

    async fn message(&self, ctx: Context, msg: Message) {
        let re = Regex::new(r"https:\/\/.+\.(facebook|instagram|tiktok)\.com[^\s]+").unwrap();

        if re.is_match(&msg.content) {
            msg.react(&ctx.http, ReactionType::Unicode("👀".to_string()))
                .await
                .unwrap();

            let caps = re.captures(&msg.content).unwrap();
            let video_url = caps.get(0).unwrap().as_str();
            println!("Matching video URL: {}", video_url);

            let request_client = Client::new();

            let resp = request_client
                .get("https://snapvideo.io/en")
                .send()
                .await
                .unwrap();
            let body = resp.text().await.unwrap();

            let snapvideo_token = spawn_blocking(move || {
                let document = Html::parse_document(&body);
                let selector = Selector::parse(r#"input[id="token"]"#).unwrap();
                let token_element = document.select(&selector).next().unwrap();
                let token = token_element.value().attr("value").unwrap();
                token.to_owned()
            })
            .await
            .unwrap();

            let form_data = [
                ("url", video_url.to_string()),
                ("token", snapvideo_token.to_string()),
                ("hash", calculate_hash(video_url, "aio-dl")),
            ];

            let video_info_res = request_client
                .post("https://snapvideo.io/wp-json/aio-dl/video-data/")
                .form(&form_data)
                .send()
                .await
                .unwrap();

            if video_info_res.status().is_success() {
                let video_info_json: serde_json::Value = video_info_res.json().await.unwrap();

                let mut media_url = video_info_json["medias"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rev()
                    .find(|media| media["extension"] == "mp4")
                    .unwrap()["url"]
                    .as_str()
                    .unwrap()
                    .to_string();

                if media_url.contains("download-local") {
                    let owned_media_url = media_url.to_string();
                    match Url::parse(&owned_media_url) {
                        Ok(url) => {
                            let download_url = url
                                .query_pairs()
                                .find(|(key, _)| key == "url")
                                .map(|(_, value)| value.into_owned());

                            if let Some(download_url) = download_url {
                                println!("Local Download URL: {}", download_url);
                                media_url = download_url;
                            }
                        }
                        Err(err) => {
                            println!("Unable to parse URL {}. Err {:?}", owned_media_url, err);
                            msg.channel_id
                                .send_message(
                                    &ctx.http,
                                    CreateMessage::new()
                                        .reference_message(&msg)
                                        .content("Unable to send video."),
                                )
                                .await
                                .unwrap();
                            return;
                        }
                    }
                }

                println!("Video URL: {}", media_url);

                let fb_media_response = request_client.get(media_url).send().await.unwrap();
                let media_bytes = fb_media_response.bytes().await.unwrap();
                let media_content =
                    CreateAttachment::bytes(media_bytes.as_ref().to_vec(), "video.mp4");

                if let Err(why) = msg
                    .channel_id
                    .send_files(
                        &ctx.http,
                        [media_content],
                        CreateMessage::new().reference_message(&msg),
                    )
                    .await
                {
                    println!("Unable to send video: {why:?}");

                    msg.channel_id
                        .send_message(
                            &ctx.http,
                            CreateMessage::new()
                                .reference_message(&msg)
                                .content("Unable to send video."),
                        )
                        .await
                        .unwrap();
                }
            } else {
                println!("Unable to get video URL {}", video_info_res.status());

                msg.channel_id
                    .send_message(
                        &ctx.http,
                        CreateMessage::new()
                            .reference_message(&msg)
                            .content("Unable to send video."),
                    )
                    .await
                    .unwrap();
            }
        }
    }
}
