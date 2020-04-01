// Copyright 2015-2020 Capital One Services, LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//
// waSCC AWS Lambda Runtime Provider
//

#[macro_use]
extern crate log;
#[macro_use]
extern crate wascc_codec;

use wascc_codec::capabilities::{CapabilityProvider, Dispatcher, NullDispatcher};
use wascc_codec::core::{CapabilityConfiguration, OP_CONFIGURE, OP_REMOVE_ACTOR};
use wascc_codec::{deserialize, serialize};
use env_logger;
use std::collections::HashMap;
use std::env;

use std::error::Error;
use std::sync::{Arc, RwLock};
use std::thread;

mod lambda;

pub const CAPABILITY_ID: &str = "awslambda:runtime";

capability_provider!(AwsLambdaRuntimeProvider, AwsLambdaRuntimeProvider::new);

// Represents a waSCC AWS Lambda runtime provider.
pub struct AwsLambdaRuntimeProvider {
    client_shutdown: Arc<RwLock<HashMap<String, bool>>>,
    dispatcher: Arc<RwLock<Box<dyn Dispatcher>>>,
}

// Represents an AWS Lambda runtime client.
struct AwsLambdaRuntimeClient {
    dispatcher: Arc<RwLock<Box<dyn Dispatcher>>>,
    module_id: String,
    runtime_client: lambda::RuntimeClient,
    shutdown: Arc<RwLock<HashMap<String, bool>>>,
}

impl Default for AwsLambdaRuntimeProvider {
    // Returns the default value for `AwsLambdaRuntimeProvider`.
    fn default() -> Self {
        if env_logger::try_init().is_err() {
            info!("Logger already intialized");
        }

        AwsLambdaRuntimeProvider {
            client_shutdown: Arc::new(RwLock::new(HashMap::new())),
            dispatcher: Arc::new(RwLock::new(Box::new(NullDispatcher::new()))),
        }
    }
}

impl AwsLambdaRuntimeProvider {
    // Creates a new, empty `AwsLambdaRuntimeProvider`.
    pub fn new() -> Self {
        Self::default()
    }

    // Starts the Lambda runtime client.
    fn start_runtime_client(&self, config: CapabilityConfiguration) -> Result<(), Box<dyn Error>> {
        info!("awslambda:runtime start_runtime_client");

        let client_shutdown = Arc::clone(&self.client_shutdown);
        let dispatcher = Arc::clone(&self.dispatcher);
        let endpoint = match config.values.get("AWS_LAMBDA_RUNTIME_API") {
            Some(ep) => String::from(ep),
            None => return Err("Missing configuration value: AWS_LAMBDA_RUNTIME_API".into()),
        };
        let module_id = config.module;
        thread::spawn(move || {
            info!("Starting runtime client for actor {}", module_id);

            // Initialize this client's shutdown flag.
            client_shutdown
                .write()
                .unwrap()
                .insert(module_id.clone(), false);

            let client =
                AwsLambdaRuntimeClient::new(&endpoint, &module_id, dispatcher, client_shutdown);
            client.run_until_shutdown();
        });

        Ok(())
    }

    // Stops any running Lambda runtime client.
    fn stop_runtime_client(&self, config: CapabilityConfiguration) -> Result<(), Box<dyn Error>> {
        info!("awslambda:runtime stop_runtime_client");

        let module_id = &config.module;
        {
            let mut lock = self.client_shutdown.write().unwrap();
            if !lock.contains_key(module_id) {
                error!(
                    "Received request to stop runtime client for unknown actor {}. Ignoring",
                    module_id
                );
                return Ok(());
            }
            *lock.get_mut(module_id).unwrap() = true;
        }
        {
            let mut lock = self.client_shutdown.write().unwrap();
            lock.remove(module_id).unwrap();
        }

        Ok(())
    }
}

impl CapabilityProvider for AwsLambdaRuntimeProvider {
    // Returns the capability ID in the formated `namespace:id`.
    fn capability_id(&self) -> &'static str {
        CAPABILITY_ID
    }

    // Called when the host runtime is ready and has configured a dispatcher.
    fn configure_dispatch(&self, dispatcher: Box<dyn Dispatcher>) -> Result<(), Box<dyn Error>> {
        info!("awslambda:runtime configure_dispatch");

        let mut lock = self.dispatcher.write().unwrap();
        *lock = dispatcher;

        Ok(())
    }

    // Called by the host runtime when an actor is requesting a command be executed.
    fn handle_call(&self, actor: &str, op: &str, msg: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        info!("awslambda:runtime handle_call `{}` from `{}`", op, actor);

        match op {
            OP_CONFIGURE if actor == "system" => self.start_runtime_client(deserialize(msg)?)?,
            OP_REMOVE_ACTOR if actor == "system" => self.stop_runtime_client(deserialize(msg)?)?,
            _ => return Err(format!("Unsupported operation: {}", op).into()),
        }

        Ok(vec![])
    }

    // Returns the human-readable, friendly name of this capability provider.
    fn name(&self) -> &'static str {
        "waSCC AWS Lambda runtime provider"
    }
}

impl AwsLambdaRuntimeClient {
    // Creates a new `AwsLambdaRuntimeClient`.
    fn new(
        endpoint: &str,
        module_id: &str,
        dispatcher: Arc<RwLock<Box<dyn Dispatcher>>>,
        shutdown: Arc<RwLock<HashMap<String, bool>>>,
    ) -> Self {
        AwsLambdaRuntimeClient {
            dispatcher,
            module_id: module_id.into(),
            runtime_client: lambda::RuntimeClient::new(endpoint),
            shutdown,
        }
    }

    // Runs until shutdown.
    fn run_until_shutdown(&self) {
        loop {
            if self.shutdown() {
                break;
            }

            // Get next event.
            debug!("AwsLambdaRuntimeClient get next event");
            let event = match self.runtime_client.next_invocation_event() {
                Err(err) => {
                    error!("{}", err);
                    continue;
                }
                Ok(evt) => match evt {
                    None => continue,
                    Some(event) => event,
                },
            };
            if event.request_id().is_none() {
                warn!("Missing request ID");
                continue;
            }

            // Set for the X-Ray SDK.
            if let Some(trace_id) = event.trace_id() {
                env::set_var("_X_AMZN_TRACE_ID", trace_id);
            }

            // Call handler.
            debug!("AwsLambdaRuntimeClient call handler");
            let handler_resp = {
                let event = codec::Event {
                    body: event.body().to_vec(),
                };
                let buf = serialize(event).unwrap();
                let lock = self.dispatcher.read().unwrap();
                lock.dispatch(
                    &format!(
                        "{}!{}",
                        &self.module_id,
                        codec::OP_HANDLE_EVENT
                    ),
                    &buf,
                )
            };
            // Handle response or error.
            match handler_resp {
                Ok(r) => {
                    let r = deserialize::<codec::Response>(r.as_slice()).unwrap();
                    let invocation_resp = lambda::InvocationResponse::new(r.body)
                        .request_id(event.request_id().unwrap());
                    debug!("AwsLambdaRuntimeClient send response");
                    match self
                        .runtime_client
                        .send_invocation_response(invocation_resp)
                    {
                        Ok(_) => {}
                        Err(err) => error!("Unable to send invocation response: {}", err),
                    }
                }
                Err(e) => {
                    error!("Guest failed to handle Lambda event: {}", e);
                    let invocation_err =
                        lambda::InvocationError::new(e).request_id(event.request_id().unwrap());
                    debug!("AwsLambdaRuntimeClient send error");
                    match self.runtime_client.send_invocation_error(invocation_err) {
                        Ok(_) => {}
                        Err(err) => error!("Unable to send invocation error: {}", err),
                    }
                }
            }
        }
    }

    // Returns whether the shutdown flag is set.
    fn shutdown(&self) -> bool {
        *self.shutdown.read().unwrap().get(&self.module_id).unwrap()
    }
}
