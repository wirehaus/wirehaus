use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, PropMode,
    SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

use crate::error::ClipboardError;
use crate::{Clipboard, MAX_TEXT_BYTES};

const READ_TIMEOUT: Duration = Duration::from_secs(2);
const MIME_UTF8: &str = "text/plain;charset=utf-8";

#[derive(Debug, Clone, Copy)]
struct Atoms {
    clipboard: u32,
    targets: u32,
    utf8: u32,
    mime_utf8: u32,
    incr: u32,
    transfer: u32,
    wake: u32,
}

#[derive(Debug)]
struct Notice {
    target: u32,
    property: u32,
}

#[derive(Debug)]
pub struct X11Clipboard {
    connection: Arc<RustConnection>,
    window: u32,
    atoms: Atoms,
    owned: Arc<Mutex<Option<String>>>,
    notices: Receiver<Notice>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl X11Clipboard {
    pub fn connect(display: Option<&str>) -> Result<Self, ClipboardError> {
        let (connection, number) =
            x11rb::connect(display).map_err(|e| ClipboardError::Connect(e.to_string()))?;
        let root = connection
            .setup()
            .roots
            .get(number)
            .ok_or_else(|| ClipboardError::Connect(format!("no screen {number}")))?
            .root;
        let connection = Arc::new(connection);
        let window = connection.generate_id().map_err(request_error)?;
        connection
            .create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_ONLY,
                0,
                &CreateWindowAux::new(),
            )
            .map_err(request_error)?;
        let atoms = intern_atoms(&connection)?;
        connection.flush().map_err(request_error)?;
        let owned = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (sender, notices) = channel();
        let worker = {
            let (connection, owned, stop) = (connection.clone(), owned.clone(), stop.clone());
            thread::spawn(move || serve(&connection, atoms, &owned, &stop, &sender))
        };
        Ok(Self {
            connection,
            window,
            atoms,
            owned,
            notices,
            stop,
            worker: Some(worker),
        })
    }

    fn request(&mut self, target: u32) -> Result<Option<Vec<u8>>, ClipboardError> {
        while self.notices.try_recv().is_ok() {}
        self.connection
            .delete_property(self.window, self.atoms.transfer)
            .map_err(request_error)?;
        self.connection
            .convert_selection(
                self.window,
                self.atoms.clipboard,
                target,
                self.atoms.transfer,
                CURRENT_TIME,
            )
            .map_err(request_error)?;
        self.connection.flush().map_err(request_error)?;
        let deadline = Instant::now() + READ_TIMEOUT;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            let notice = self
                .notices
                .recv_timeout(wait)
                .map_err(|_| ClipboardError::Timeout)?;
            if notice.target != target {
                continue;
            }
            return if notice.property == NONE {
                Ok(None)
            } else {
                self.take_property().map(Some)
            };
        }
    }

    fn take_property(&self) -> Result<Vec<u8>, ClipboardError> {
        let words = u32::try_from(MAX_TEXT_BYTES / 4 + 1).unwrap_or(u32::MAX);
        let reply = self
            .connection
            .get_property(
                true,
                self.window,
                self.atoms.transfer,
                AtomEnum::ANY,
                0,
                words,
            )
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?;
        if reply.type_ == self.atoms.incr
            || reply.bytes_after > 0
            || reply.value.len() > MAX_TEXT_BYTES
        {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        Ok(reply.value)
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.owned.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clipboard for X11Clipboard {
    fn read(&mut self) -> Result<Option<String>, ClipboardError> {
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?
            .owner;
        if owner == NONE {
            return Ok(None);
        }
        if owner == self.window {
            return Ok(self.locked().clone());
        }
        let bytes = match self.request(self.atoms.utf8)? {
            Some(bytes) => bytes,
            None => match self.request(u32::from(AtomEnum::STRING))? {
                Some(bytes) => bytes
                    .into_iter()
                    .map(char::from)
                    .collect::<String>()
                    .into_bytes(),
                None => return Ok(None),
            },
        };
        match String::from_utf8(bytes) {
            Ok(text) if text.is_empty() => Ok(None),
            Ok(text) => Ok(Some(text)),
            Err(_) => Ok(None),
        }
    }

    fn write(&mut self, text: &str) -> Result<(), ClipboardError> {
        if text.len() > MAX_TEXT_BYTES {
            return Err(ClipboardError::TooLarge(MAX_TEXT_BYTES));
        }
        *self.locked() = Some(text.to_owned());
        self.connection
            .set_selection_owner(self.window, self.atoms.clipboard, CURRENT_TIME)
            .map_err(request_error)?;
        let owner = self
            .connection
            .get_selection_owner(self.atoms.clipboard)
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?
            .owner;
        if owner == self.window {
            Ok(())
        } else {
            *self.locked() = None;
            Err(ClipboardError::NotOwner)
        }
    }
}

impl Drop for X11Clipboard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let wake = ClientMessageEvent::new(32, self.window, self.atoms.wake, [0_u32; 5]);
        let _ = self
            .connection
            .send_event(false, self.window, EventMask::NO_EVENT, wake);
        let _ = self.connection.flush();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn serve(
    connection: &RustConnection,
    atoms: Atoms,
    owned: &Mutex<Option<String>>,
    stop: &AtomicBool,
    notices: &Sender<Notice>,
) {
    while !stop.load(Ordering::SeqCst) {
        let Ok(event) = connection.wait_for_event() else {
            return;
        };
        match event {
            Event::SelectionRequest(request) => {
                let text = owned.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let _ = answer(connection, atoms, &request, text.as_deref());
            }
            Event::SelectionClear(clear) if clear.selection == atoms.clipboard => {
                *owned.lock().unwrap_or_else(PoisonError::into_inner) = None;
            }
            Event::SelectionNotify(notify) => {
                let _ = notices.send(Notice {
                    target: notify.target,
                    property: notify.property,
                });
            }
            _ => {}
        }
    }
}

fn answer(
    connection: &RustConnection,
    atoms: Atoms,
    request: &SelectionRequestEvent,
    text: Option<&str>,
) -> Result<(), ClipboardError> {
    let property = if request.property == NONE {
        request.target
    } else {
        request.property
    };
    let served = match text {
        Some(text) if request.selection == atoms.clipboard => {
            fill(connection, atoms, request, property, text)?
        }
        _ => false,
    };
    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property: if served { property } else { NONE },
    };
    connection
        .send_event(false, request.requestor, EventMask::NO_EVENT, notify)
        .map_err(request_error)?;
    connection.flush().map_err(request_error)
}

fn fill(
    connection: &RustConnection,
    atoms: Atoms,
    request: &SelectionRequestEvent,
    property: u32,
    text: &str,
) -> Result<bool, ClipboardError> {
    let target = request.target;
    if target == atoms.targets {
        let supported = [
            atoms.targets,
            atoms.utf8,
            atoms.mime_utf8,
            u32::from(AtomEnum::STRING),
        ];
        connection
            .change_property32(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::ATOM,
                &supported,
            )
            .map_err(request_error)?;
        Ok(true)
    } else if target == atoms.utf8 || target == atoms.mime_utf8 {
        connection
            .change_property8(
                PropMode::REPLACE,
                request.requestor,
                property,
                target,
                text.as_bytes(),
            )
            .map_err(request_error)?;
        Ok(true)
    } else if target == u32::from(AtomEnum::STRING) && text.is_ascii() {
        connection
            .change_property8(
                PropMode::REPLACE,
                request.requestor,
                property,
                AtomEnum::STRING,
                text.as_bytes(),
            )
            .map_err(request_error)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn intern_atoms(connection: &RustConnection) -> Result<Atoms, ClipboardError> {
    let intern = |name: &str| -> Result<u32, ClipboardError> {
        Ok(connection
            .intern_atom(false, name.as_bytes())
            .map_err(request_error)?
            .reply()
            .map_err(request_error)?
            .atom)
    };
    Ok(Atoms {
        clipboard: intern("CLIPBOARD")?,
        targets: intern("TARGETS")?,
        utf8: intern("UTF8_STRING")?,
        mime_utf8: intern(MIME_UTF8)?,
        incr: intern("INCR")?,
        transfer: intern("WIREHAUS_CLIPBOARD")?,
        wake: intern("WIREHAUS_WAKE")?,
    })
}

fn request_error(error: impl std::fmt::Display) -> ClipboardError {
    ClipboardError::Request(error.to_string())
}
