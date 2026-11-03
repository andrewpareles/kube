use uuid::Uuid;

// ---------------------------------------------- Deep Link Parsing ----------------------------------------------

#[derive(Debug)]
pub enum DeepLinkParams {
    Play {
        // auth
        play_auth: String,
        // info
        request_id: String, // the reserved server to join

        // only used to get thumbnail, nothing else
        thumbnail_uuid: Option<Uuid>,
        // shown on the loading screen, nothing else
        thumbnail_game_name: Option<String>,
    },
    Playtest {
        public_ip: String,
        port: u16,
        join_token: String,
    },
    Blank,
}

pub enum DeepLinkParseError {
    MissingMode,
    InvalidMode,
    MissingField(&'static str),
    InvalidField(&'static str),
}

pub fn parse_deep_link(deep_link: &url::Url) -> anyhow::Result<DeepLinkParams, DeepLinkParseError> {
    let mut mode = None;
    let mut join_token = None;
    let mut play_auth = None;
    let mut request_id = None;
    let mut public_ip = None;
    let mut port = None;
    let mut thumbnail_uuid = None;
    let mut thumbnail_game_name = None;

    for (key, value) in deep_link.query_pairs() {
        let key = key.as_ref();
        match key {
            "join_token" => join_token = Some(value.to_string()),
            "mode" => mode = Some(value.to_string()),
            "play_auth" => play_auth = Some(value.to_string()),
            "request_id" => request_id = Some(value.to_string()),
            "public_ip" => public_ip = Some(value.to_string()),
            "port" => port = Some(value.to_string()),
            "thumbnail_uuid" => thumbnail_uuid = Uuid::parse_str(&value).ok(), // ignore errors
            "thumbnail_game_name" => thumbnail_game_name = Some(value.to_string()),
            _ => continue,
        }
    }

    let mode = mode.ok_or(DeepLinkParseError::MissingMode)?;

    match mode.as_str() {
        "play" => {
            let play_auth = play_auth.ok_or(DeepLinkParseError::MissingField("play_auth"))?;
            let request_id = request_id.ok_or(DeepLinkParseError::MissingField("request_id"))?;
            Ok(DeepLinkParams::Play {
                request_id,
                play_auth,
                thumbnail_uuid,
                thumbnail_game_name,
            })
        }
        "playtest" => Ok(DeepLinkParams::Playtest {
            public_ip: public_ip
                .filter(|value| !value.is_empty())
                .ok_or(DeepLinkParseError::MissingField("public_ip"))?,
            port: port
                .ok_or(DeepLinkParseError::MissingField("port"))?
                .parse::<std::num::NonZeroU16>()
                .map_err(|_| DeepLinkParseError::InvalidField("port"))?
                .get(),
            join_token: join_token
                .filter(|value| !value.is_empty())
                .ok_or(DeepLinkParseError::MissingField("join_token"))?,
        }),
        "blank" => Ok(DeepLinkParams::Blank),
        _ => Err(DeepLinkParseError::InvalidMode),
    }
}
