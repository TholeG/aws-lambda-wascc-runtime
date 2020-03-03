//
// waSCC AWS Lambda Runtime Provider
//

use serde_json;
use std::error::Error;

// Represents an invocation event.
pub struct LambdaInvocationEvent {
    body: Vec<u8>,
    request_id: Option<String>,
    trace_id: Option<String>,
}

// Represents an invocation response.
pub struct LambdaInvocationResponse {
    body: Vec<u8>,
    request_id: Option<String>,
}

// Represents an invocation error.
pub struct LambdaInvocationError {
    error: Box<dyn Error>,
    request_id: Option<String>,
}

// Represents an AWS Lambda runtime HTTP client.
pub struct LambdaRuntimeClient {
    endpoint: String,
    http_client: reqwest::blocking::Client,
}

impl LambdaRuntimeClient {
    // Creates a new `LambdaRuntimeClient` with the specified AWS Lambda runtime API endpoint.
    pub fn new(endpoint: &str) -> Self {
        LambdaRuntimeClient {
            endpoint: endpoint.into(),
            http_client: reqwest::blocking::Client::new(),
        }
    }

    // Returns the next AWS Lambda invocation event.
    pub fn next_invocation_event(&self) -> Result<Option<LambdaInvocationEvent>, Box<dyn Error>> {
        // https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-next
        let url = format!(
            "http://{}/2018-06-01/runtime/invocation/next",
            self.endpoint
        );
        let mut resp = self.http_client.get(&url).send()?;
        let status = resp.status();
        info!(
            "GET {} {} {}",
            url,
            status.as_str(),
            status.canonical_reason().unwrap()
        );
        if !status.is_success() {
            return Ok(None);
        }

        let mut buf: Vec<u8> = vec![];
        resp.copy_to(&mut buf)?;
        let mut event = LambdaInvocationEvent::new(buf);
        if let Some(request_id) = resp.headers().get("Lambda-Runtime-Aws-Request-Id") {
            event.request_id = Some(request_id.to_str()?.into());
        }
        if let Some(trace_id) = resp.headers().get("Lambda-Runtime-Trace-Id") {
            event.trace_id = Some(trace_id.to_str()?.into());
        }

        Ok(Some(event))
    }

    // Sends an invocation error to the AWS Lambda runtime.
    pub fn send_invocation_error(
        &self,
        error: LambdaInvocationError,
    ) -> Result<(), Box<dyn Error>> {
        if error.request_id.is_none() {
            warn!("No request ID specified. Unable to send invocation error");
            return Ok(());
        }

        // https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-invokeerror
        let url = format!(
            "http://{}/2018-06-01/runtime/invocation/{}/error",
            self.endpoint,
            error.request_id.unwrap()
        );
        let resp = self
            .http_client
            .post(&url)
            .json(&serde_json::json!({
                "errorMessage": error.error.to_string(),
            }))
            .send()?;
        let status = resp.status();
        info!(
            "POST {} {} {}",
            url,
            status.as_str(),
            status.canonical_reason().unwrap()
        );

        Ok(())
    }

    // Sends an invocation response to the AWS Lambda runtime.
    pub fn send_invocation_response(
        &self,
        resp: LambdaInvocationResponse,
    ) -> Result<(), Box<dyn Error>> {
        if resp.request_id.is_none() {
            warn!("No request ID specified. Unable to send invocation response");
            return Ok(());
        }

        // https://docs.aws.amazon.com/lambda/latest/dg/runtimes-api.html#runtimes-api-response
        let url = format!(
            "http://{}/2018-06-01/runtime/invocation/{}/response",
            self.endpoint,
            resp.request_id.unwrap()
        );
        let resp = self.http_client.post(&url).body(resp.body).send()?;
        let status = resp.status();
        info!(
            "POST {} {} {}",
            url,
            status.as_str(),
            status.canonical_reason().unwrap()
        );

        Ok(())
    }
}

impl LambdaInvocationEvent {
    // Creates a new `LambdaInvocationEvent` with the specified body.
    pub fn new(body: Vec<u8>) -> Self {
        LambdaInvocationEvent {
            body: body,
            request_id: None,
            trace_id: None,
        }
    }

    // Returns the event body.
    pub fn body(&self) -> &Vec<u8> {
        self.body.as_ref()
    }

    // Returns any request ID.
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    // Returns any trace ID.
    pub fn trace_id(&self) -> Option<&str> {
        self.trace_id.as_deref()
    }
}

impl LambdaInvocationResponse {
    // Creates a new `LambdaInvocationResponse` with the specified body.
    pub fn new(body: Vec<u8>) -> Self {
        LambdaInvocationResponse {
            body: body,
            request_id: None,
        }
    }

    // Creates a new `LambdaInvocationResponse` with the specified request ID.
    pub fn request_id(self, request_id: &str) -> Self {
        LambdaInvocationResponse {
            body: self.body,
            request_id: Some(request_id.into()),
        }
    }
}

impl LambdaInvocationError {
    // Creates a new `LambdaInvocationError` with the specified error.
    pub fn new(error: Box<dyn Error>) -> Self {
        LambdaInvocationError {
            error: error,
            request_id: None,
        }
    }

    // Creates a new `LambdaInvocationError` with the specified request ID.
    pub fn request_id(self, request_id: &str) -> Self {
        LambdaInvocationError {
            error: self.error,
            request_id: Some(request_id.into()),
        }
    }
}
