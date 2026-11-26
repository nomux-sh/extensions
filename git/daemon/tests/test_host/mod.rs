//! A minimal host for the test that runs the program of the extension. It
//! starts the program as the nomux daemon does and speaks the extension
//! protocol on the pipes of the process. The real host is the nomux daemon.

use std::collections::{HashMap, VecDeque};
use std::io::BufReader;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use nomux_extension::wire::v1::envelope::Kind;
use nomux_extension::wire::v1::request::Body;
use nomux_extension::wire::v1::{
    CallRequest, Envelope, Error, Event, Hello, InitializeRequest, Request, Response,
    ShutdownRequest, response,
};
use nomux_extension::{ApiVersion, Permissions, frame};
use prost::Message;

/// The longest time that the host waits for one message of the extension.
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(20);

pub struct TestHost {
    child: Child,
    input: ChildStdin,
    messages: Receiver<Kind>,
    last_id: u64,
    responses: HashMap<u64, response::Result>,
    events: VecDeque<Event>,
}

impl TestHost {
    /// Starts the program and reads its messages on a thread, thus a program
    /// that does not answer fails the test and does not block it.
    pub fn start(program: &str) -> Self {
        let mut child = Command::new(program)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (sender, messages) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(Some(Envelope { kind: Some(kind) })) = frame::read_envelope(&mut output) {
                if sender.send(kind).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            input,
            messages,
            last_id: 0,
            responses: HashMap::new(),
            events: VecDeque::new(),
        }
    }

    pub fn request(&mut self, body: Body) -> response::Result {
        self.last_id += 1;
        let request = Request {
            id: self.last_id,
            body: Some(body),
        };
        let envelope = Envelope {
            kind: Some(Kind::Request(request)),
        };
        frame::write_envelope(&mut self.input, &envelope).unwrap();
        loop {
            if let Some(result) = self.responses.remove(&self.last_id) {
                return result;
            }
            self.read();
        }
    }

    /// Does the handshake and the initialization and returns the reply to
    /// `Hello`.
    pub fn hello(&mut self, extension_id: &str, permissions: Permissions) -> response::Result {
        let welcome = self.request(Body::Hello(Hello {
            api_version: Some(ApiVersion::CURRENT),
            extension_id: extension_id.to_owned(),
            permissions: Some(permissions),
        }));
        self.request(Body::Initialize(InitializeRequest {}));
        welcome
    }

    pub fn call<Reply: Message + Default>(
        &mut self,
        method: &str,
        input: &impl Message,
    ) -> Result<Reply, Error> {
        let body = Body::Call(CallRequest {
            method: method.to_owned(),
            payload: input.encode_to_vec(),
        });
        match self.request(body) {
            response::Result::Call(reply) => Ok(Reply::decode(reply.payload.as_slice()).unwrap()),
            response::Result::Error(error) => Err(error),
            other => panic!("the response is not a call result: {other:?}"),
        }
    }

    pub fn next_event(&mut self) -> Event {
        loop {
            if let Some(event) = self.events.pop_front() {
                return event;
            }
            self.read();
        }
    }

    /// Sends the shutdown request and waits for the end of the process.
    pub fn shutdown(mut self) -> ExitStatus {
        self.request(Body::Shutdown(ShutdownRequest {}));
        self.child.wait().unwrap()
    }

    fn read(&mut self) {
        let message = self
            .messages
            .recv_timeout(MESSAGE_TIMEOUT)
            .expect("the extension sent no message");
        match message {
            Kind::Response(Response {
                id,
                result: Some(result),
            }) => {
                self.responses.insert(id, result);
            }
            Kind::Event(event) => self.events.push_back(event),
            other => {
                panic!("the extension sent a message that the test does not expect: {other:?}")
            }
        }
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        // A test that failed leaves the process. `kill` fails only when the
        // process already ended.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
