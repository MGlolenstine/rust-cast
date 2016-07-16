use std::borrow::Cow;
use std::convert::Into;
use std::io::{Read, Write};
use std::rc::Rc;
use std::str::FromStr;
use std::string::ToString;

use serde_json;

use cast::proxies;
use errors::Error;
use message_manager::{CastMessage, CastMessagePayload, MessageManager};

const CHANNEL_NAMESPACE: &'static str = "urn:x-cast:com.google.cast.receiver";

const MESSAGE_TYPE_LAUNCH: &'static str = "LAUNCH";
const MESSAGE_TYPE_STOP: &'static str = "STOP";
const MESSAGE_TYPE_GET_STATUS: &'static str = "GET_STATUS";
const MESSAGE_TYPE_SET_VOLUME: &'static str = "SET_VOLUME";

const MESSAGE_TYPE_RECEIVER_STATUS: &'static str = "RECEIVER_STATUS";
const MESSAGE_TYPE_LAUNCH_ERROR: &'static str = "LAUNCH_ERROR";
const MESSAGE_TYPE_INVALID_REQUEST: &'static str = "INVALID_REQUEST";

const APP_DEFAULT_MEDIA_RECEIVER_ID: &'static str = "CC1AD845";
const APP_BACKDROP_ID: &'static str = "E8C28D3C";
const APP_YOUTUBE_ID: &'static str = "233637DE";

/// Structure that describes possible cast device volume options.
#[derive(Debug)]
pub struct Volume {
    /// Volume level.
    pub level: Option<f32>,
    /// Mute/unmute state.
    pub muted: Option<bool>,
}

/// This `Into<Volume>` implementation is useful when only volume level is needed.
impl Into<Volume> for f32 {
    fn into(self) -> Volume {
        Volume {
            level: Some(self),
            muted: None,
        }
    }
}

/// This `Into<Volume>` implementation is useful when only mute/unmute state is needed.
impl Into<Volume> for bool {
    fn into(self) -> Volume {
        Volume {
            level: None,
            muted: Some(self),
        }
    }
}

/// This `Into<Volume>` implementation is useful when both volume level and mute/unmute state are
/// needed.
impl Into<Volume> for (f32, bool) {
    fn into(self) -> Volume {
        Volume {
            level: Some(self.0),
            muted: Some(self.1),
        }
    }
}

/// Structure that describes currently run Cast Device application.
#[derive(Debug)]
pub struct Application {
    /// The identifier of the Cast application. Not for display.
    pub app_id: String,
    /// Session id of the currently active application.
    pub session_id: String,
    /// Name of the `pipe` to talk to the application.
    pub transport_id: String,
    /// A list of the namespaces supported by the receiver application.
    pub namespaces: Vec<String>,
    /// The human-readable name of the Cast application, for example, "YouTube".
    pub display_name: String,
    /// Descriptive text for the current application content, for example “My vacations”.
    pub status_text: String,
}

/// Describes the current status of the receiver cast device.
#[derive(Debug)]
pub struct Status {
    /// Unique id of the request that requested the status.
    pub request_id: i32,
    /// Contains the list of applications that are currently run.
    pub applications: Vec<Application>,
    /// Determines whether the Cast device is the active input or not.
    pub is_active_input: bool,
    /// Determines whether the Cast device is in stand by mode.
    pub is_stand_by: bool,
    /// Volume parameters of the currently active cast device.
    pub volume: Volume,
}

/// Describes the application launch error.
#[derive(Debug)]
pub struct LaunchError {
    /// Unique id of the request that tried to launch application.
    pub request_id: i32,
    /// Description of the launch error reason if available.
    pub reason: Option<String>,
}

/// Describes the invalid request error.
#[derive(Debug)]
pub struct InvalidRequest {
    /// Unique id of the invalid request.
    pub request_id: i32,
    /// Description of the invalid request reason if available.
    pub reason: Option<String>,
}

/// Represents all currently supported incoming messages that receiver channel can handle.
#[derive(Debug)]
pub enum ReceiverResponse {
    /// Status of the currently active receiver.
    Status(Status),
    /// Error indicating that receiver failed to launch application.
    LaunchError(LaunchError),
    /// Error indicating that request is not valid.
    InvalidRequest(InvalidRequest),
    /// Used every time when channel can't parse the message. Associated data contains `type` string
    /// field and raw JSON data returned from cast device.
    NotImplemented(String, serde_json::Value),
}

#[derive(Debug, PartialEq, Clone)]
pub enum CastDeviceApp {
    DefaultMediaReceiver,
    Backdrop,
    YouTube,
    Custom(String),
}

impl FromStr for CastDeviceApp {
    type Err = ();

    fn from_str(s: &str) -> Result<CastDeviceApp, ()> {
        let app = match s {
            APP_DEFAULT_MEDIA_RECEIVER_ID | "default" => CastDeviceApp::DefaultMediaReceiver,
            APP_BACKDROP_ID | "backdrop" => CastDeviceApp::Backdrop,
            APP_YOUTUBE_ID | "youtube" => CastDeviceApp::YouTube,
            custom @ _ => CastDeviceApp::Custom(custom.to_owned())
        };

        Ok(app)
    }
}

impl ToString for CastDeviceApp {
    fn to_string(&self) -> String {
        match *self {
            CastDeviceApp::DefaultMediaReceiver => APP_DEFAULT_MEDIA_RECEIVER_ID.to_owned(),
            CastDeviceApp::Backdrop => APP_BACKDROP_ID.to_owned(),
            CastDeviceApp::YouTube => APP_YOUTUBE_ID.to_owned(),
            CastDeviceApp::Custom(ref app_id) => app_id.to_owned(),
        }
    }
}

pub struct ReceiverChannel<'a, W> where W: Write + Read {
    sender: Cow<'a, str>,
    receiver: Cow<'a, str>,
    message_manager: Rc<MessageManager<W>>,
}

impl<'a, W> ReceiverChannel<'a, W> where W: Write + Read {
    pub fn new<S>(sender: S, receiver: S, message_manager: Rc<MessageManager<W>>)
        -> ReceiverChannel<'a, W> where S: Into<Cow<'a, str>> {
        ReceiverChannel {
            sender: sender.into(),
            receiver: receiver.into(),
            message_manager: message_manager,
        }
    }

    /// Launches the specified receiver's application.
    ///
    /// # Examples
    ///
    /// ```
    /// cast_device.receiver.launch_app(&CastDeviceApp::from_str("youtube").unwrap());
    /// ```
    ///
    /// # Arguments
    ///
    /// * `app` - `CastDeviceApp` instance reference to run.
    pub fn launch_app(&self, app: &CastDeviceApp) -> Result<Application, Error> {
        let request_id = self.message_manager.generate_request_id();

        let payload = try!(serde_json::to_string(
            &proxies::receiver::AppLaunchRequest {
                typ: MESSAGE_TYPE_LAUNCH.to_owned(),
                request_id: request_id,
                app_id: app.to_string(),
            }));

        try!(self.message_manager.send(CastMessage {
            namespace: CHANNEL_NAMESPACE.to_owned(),
            source: self.sender.to_string(),
            destination: self.receiver.to_string(),
            payload: CastMessagePayload::String(payload),
        }));

        // Once application is run cast receiver device should emit status update event, or launch
        // error event if something went wrong.
        self.message_manager.receive_find_map(|message| {
            if !self.can_handle(message) {
                return Ok(None);
            }

            match try!(self.parse(message)) {
                ReceiverResponse::Status(mut status) => {
                    if status.request_id == request_id {
                        return Ok(Some(status.applications.remove(0)));
                    }
                },
                ReceiverResponse::LaunchError(error) => {
                    if error.request_id == request_id {
                        return Err(Error::Internal(
                            format!("Could not run application ({}).",
                                    error.reason.unwrap_or("Unknown".to_owned())))
                        );
                    }
                },
                _ => {}
            }

            return Ok(None);
        })
    }

    /// Stops currently active app using corresponding `session_id`.
    ///
    /// # Arguments
    /// * `session_id` - identifier of the active application session from `Application` instance.
    pub fn stop_app<S>(&self, session_id: S) -> Result<(), Error> where S: Into<Cow<'a, str>> {
        let request_id = self.message_manager.generate_request_id();

        let payload = try!(serde_json::to_string(
            &proxies::receiver::AppStopRequest {
                typ: MESSAGE_TYPE_STOP.to_owned(),
                request_id: request_id,
                session_id: session_id.into(),
            }));

        try!(self.message_manager.send(CastMessage {
            namespace: CHANNEL_NAMESPACE.to_owned(),
            source: self.sender.to_string(),
            destination: self.receiver.to_string(),
            payload: CastMessagePayload::String(payload),
        }));

        // Once application is stopped cast receiver device should emit status update event, or
        // invalid request event if provided session id is not valid.
        self.message_manager.receive_find_map(|message| {
            if !self.can_handle(message) {
                return Ok(None);
            }

            match try!(self.parse(message)) {
                ReceiverResponse::Status(status) => {
                    if status.request_id == request_id {
                        return Ok(Some(()));
                    }
                },
                ReceiverResponse::InvalidRequest(error) => {
                    if error.request_id == request_id {
                        return Err(Error::Internal(
                            format!("Invalid request ({}).",
                                    error.reason.unwrap_or("Unknown".to_owned())))
                        );
                    }
                },
                _ => {}
            }

            return Ok(None);
        })
    }

    /// Retrieves status of the cast device receiver.
    ///
    /// # Return value
    ///
    /// Returned `Result` should consist of either `Status` instance or an `Error`.
    pub fn get_status(&self) -> Result<Status, Error> {
        let request_id = self.message_manager.generate_request_id();

        let payload = try!(serde_json::to_string(
            &proxies::receiver::GetStatusRequest {
                typ: MESSAGE_TYPE_GET_STATUS.to_owned(),
                request_id: request_id,
            }));

        try!(self.message_manager.send(
            CastMessage {
                namespace: CHANNEL_NAMESPACE.to_owned(),
                source: self.sender.to_string(),
                destination: self.receiver.to_string(),
                payload: CastMessagePayload::String(payload),
            }
        ));

        self.message_manager.receive_find_map(|message| {
            if !self.can_handle(message) {
                return Ok(None);
            }

            let message = try!(self.parse(message));
            if let ReceiverResponse::Status(status) = message {
                if status.request_id == request_id {
                    return Ok(Some(status));
                }
            }

            return Ok(None);
        })
    }

    /// Sets volume for the active cast device.
    ///
    /// # Arguments
    ///
    /// * `volume` - anything that can be converted to a valid `Volume` structure. It's possible to
    ///              set volume level, mute/unmute state or both altogether.
    ///
    /// # Return value
    ///
    /// Actual `Volume` instance returned by receiver.
    ///
    /// # Errors
    ///
    /// Usually method can fail only if network connection with cast device is lost for some reason.
    pub fn set_volume<T>(&self, volume: T) -> Result<Volume, Error> where T: Into<Volume> {
        let request_id = self.message_manager.generate_request_id();
        let volume = volume.into();

        let payload = try!(serde_json::to_string(
            &proxies::receiver::SetVolumeRequest {
                typ: MESSAGE_TYPE_SET_VOLUME.to_owned(),
                request_id: request_id,
                volume: proxies::receiver::Volume {
                    level: volume.level,
                    muted: volume.muted,
                },
            }));

        try!(self.message_manager.send(CastMessage {
            namespace: CHANNEL_NAMESPACE.to_owned(),
            source: self.sender.to_string(),
            destination: self.receiver.to_string(),
            payload: CastMessagePayload::String(payload),
        }));

        self.message_manager.receive_find_map(|message| {
            if !self.can_handle(message) {
                return Ok(None);
            }

            let message = try!(self.parse(message));
            if let ReceiverResponse::Status(status) = message {
                if status.request_id == request_id {
                    return Ok(Some(status.volume));
                }
            }

            return Ok(None);
        })
    }

    pub fn can_handle(&self, message: &CastMessage) -> bool {
        message.namespace == CHANNEL_NAMESPACE
    }

    pub fn parse(&self, message: &CastMessage) -> Result<ReceiverResponse, Error> {
        let reply = match message.payload {
            CastMessagePayload::String(ref payload) => try!(
                serde_json::from_str::<serde_json::Value>(payload)),
            _ => return Err(Error::Internal("Binary payload is not supported!".to_owned())),
        };

        let message_type = reply.as_object()
            .and_then(|object| object.get("type"))
            .and_then(|property| property.as_string())
            .unwrap_or("")
            .to_owned();

        let response = match message_type.as_ref() {
            MESSAGE_TYPE_RECEIVER_STATUS => {
                let status_reply: proxies::receiver::StatusReply = try!(
                    serde_json::value::from_value(reply));

                let status = Status {
                    request_id: status_reply.request_id,
                    applications: status_reply.status.applications.iter().map(|ref app| {
                        Application {
                            app_id: app.app_id.clone(),
                            session_id: app.session_id.clone(),
                            transport_id: app.transport_id.clone(),
                            namespaces: app.namespaces.iter().map(|ref ns| ns.name.clone())
                                .collect::<Vec<String>>(),
                            display_name: app.display_name.clone(),
                            status_text: app.status_text.clone(),
                        }
                    }).collect::<Vec<Application>>(),
                    is_active_input: status_reply.status.is_active_input,
                    is_stand_by: status_reply.status.is_stand_by,
                    volume: Volume {
                        level: status_reply.status.volume.level,
                        muted: status_reply.status.volume.muted,
                    },
                };

                ReceiverResponse::Status(status)
            },
            MESSAGE_TYPE_LAUNCH_ERROR => {
                let reply: proxies::receiver::LaunchErrorReply = try!(
                    serde_json::value::from_value(reply));

                ReceiverResponse::LaunchError(LaunchError {
                    request_id: reply.request_id,
                    reason: reply.reason,
                })
            },
            MESSAGE_TYPE_INVALID_REQUEST => {
                let reply: proxies::receiver::InvalidRequestReply = try!(
                    serde_json::value::from_value(reply));

                ReceiverResponse::InvalidRequest(InvalidRequest {
                    request_id: reply.request_id,
                    reason: reply.reason,
                })
            },
            _ => ReceiverResponse::NotImplemented(message_type.to_owned(), reply),
        };

        Ok(response)
    }
}
