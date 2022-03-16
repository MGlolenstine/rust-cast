use std::io::{Read, Write};

use crate::{
    cast::proxies,
    errors::Error,
    message_manager::{CastMessage, CastMessagePayload, MessageManager},
    Lrc,
};

const CHANNEL_NAMESPACE: &str = "urn:x-cast:com.google.cast.tp.connection";
const CHANNEL_USER_AGENT: &str = "RustCast";

const MESSAGE_TYPE_CONNECT: &str = "CONNECT";
const MESSAGE_TYPE_CLOSE: &str = "CLOSE";

#[derive(Clone, Debug)]
pub enum ConnectionResponse {
    Connect,
    Close,
    NotImplemented(String, serde_json::Value),
}

pub struct ConnectionChannel<W>
where
    W: Read + Write,
{
    sender: String,
    message_manager: Lrc<MessageManager<W>>,
}

impl<W> ConnectionChannel<W>
where
    W: Read + Write,
{
    pub fn new<S>(sender: S, message_manager: Lrc<MessageManager<W>>) -> ConnectionChannel<W>
    where
        S: Into<String>,
    {
        ConnectionChannel {
            sender: sender.into(),
            message_manager,
        }
    }

    pub fn connect<S>(&self, destination: S) -> Result<(), Error>
    where
        S: Into<String>,
    {
        let payload = serde_json::to_string(&proxies::connection::ConnectionRequest {
            typ: MESSAGE_TYPE_CONNECT.to_string(),
            user_agent: CHANNEL_USER_AGENT.to_string(),
        })?;

        self.message_manager.send(CastMessage {
            namespace: CHANNEL_NAMESPACE.to_string(),
            source: self.sender.to_string(),
            destination: destination.into(),
            payload: CastMessagePayload::String(payload),
        })
    }

    pub fn disconnect<S>(&self, destination: S) -> Result<(), Error>
    where
        S: Into<String>,
    {
        let payload = serde_json::to_string(&proxies::connection::ConnectionRequest {
            typ: MESSAGE_TYPE_CLOSE.to_string(),
            user_agent: CHANNEL_USER_AGENT.to_string(),
        })?;

        self.message_manager.send(CastMessage {
            namespace: CHANNEL_NAMESPACE.to_string(),
            source: self.sender.to_string(),
            destination: destination.into(),
            payload: CastMessagePayload::String(payload),
        })
    }

    pub fn can_handle(&self, message: &CastMessage) -> bool {
        message.namespace == CHANNEL_NAMESPACE
    }

    pub fn parse(&self, message: &CastMessage) -> Result<ConnectionResponse, Error> {
        let reply = match message.payload {
            CastMessagePayload::String(ref payload) => {
                serde_json::from_str::<serde_json::Value>(payload)?
            }
            _ => {
                return Err(Error::Internal(
                    "Binary payload is not supported!".to_string(),
                ))
            }
        };

        let message_type = reply
            .as_object()
            .and_then(|object| object.get("type"))
            .and_then(|property| property.as_str())
            .unwrap_or("")
            .to_string();

        let response = match message_type.as_ref() {
            MESSAGE_TYPE_CONNECT => ConnectionResponse::Connect,
            MESSAGE_TYPE_CLOSE => ConnectionResponse::Close,
            _ => ConnectionResponse::NotImplemented(message_type.to_string(), reply),
        };

        Ok(response)
    }
}
