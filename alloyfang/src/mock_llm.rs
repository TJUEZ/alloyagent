//! Mock LLM driver for benchmarking without API keys.
//!
//! Provides pre-scripted responses with configurable latency to simulate
//! LLM interactions in benchmarks.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

/// A pre-scripted LLM response.
#[derive(Debug, Clone)]
pub struct MockResponse {
    pub text: String,
    pub tool_calls: Vec<MockToolCall>,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A mock tool call from the LLM.
#[derive(Debug, Clone)]
pub struct MockToolCall {
    pub name: String,
    pub input: serde_json::Value,
}

/// Mock LLM driver that returns pre-scripted responses.
pub struct MockLlmDriver {
    responses: Mutex<VecDeque<MockResponse>>,
    latency: Duration,
    calls_made: Mutex<u64>,
}

impl MockLlmDriver {
    /// Create a driver that returns text-only responses.
    pub fn text_only(text: &str, latency: Duration) -> Self {
        let mut queue = VecDeque::new();
        queue.push_back(MockResponse {
            text: text.to_owned(),
            tool_calls: vec![],
            input_tokens: 100,
            output_tokens: 50,
        });
        Self {
            responses: Mutex::new(queue),
            latency,
            calls_made: Mutex::new(0),
        }
    }

    /// Create a driver with a sequence of responses (cycles when exhausted).
    pub fn with_responses(responses: Vec<MockResponse>, latency: Duration) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            latency,
            calls_made: Mutex::new(0),
        }
    }

    /// Create a driver that first returns a tool call, then a text response.
    pub fn single_tool_then_text(
        tool_name: &str,
        tool_input: serde_json::Value,
        final_text: &str,
    ) -> Self {
        let responses = vec![
            MockResponse {
                text: String::new(),
                tool_calls: vec![MockToolCall {
                    name: tool_name.to_owned(),
                    input: tool_input,
                }],
                input_tokens: 150,
                output_tokens: 30,
            },
            MockResponse {
                text: final_text.to_owned(),
                tool_calls: vec![],
                input_tokens: 200,
                output_tokens: 50,
            },
        ];
        Self::with_responses(responses, Duration::from_millis(50))
    }

    /// Create a driver that returns multiple parallel tool calls.
    pub fn parallel_tools(tools: Vec<(String, serde_json::Value)>, final_text: &str) -> Self {
        let tool_calls: Vec<MockToolCall> = tools
            .into_iter()
            .map(|(name, input)| MockToolCall { name, input })
            .collect();
        let responses = vec![
            MockResponse {
                text: String::new(),
                tool_calls,
                input_tokens: 200,
                output_tokens: 80,
            },
            MockResponse {
                text: final_text.to_owned(),
                tool_calls: vec![],
                input_tokens: 300,
                output_tokens: 50,
            },
        ];
        Self::with_responses(responses, Duration::from_millis(50))
    }

    /// Simulate a completion call.
    pub async fn complete(&self) -> MockResponse {
        // Simulate latency.
        tokio::time::sleep(self.latency).await;

        let mut calls = self.calls_made.lock().unwrap();
        *calls += 1;

        let mut queue = self.responses.lock().unwrap();
        if let Some(response) = queue.pop_front() {
            // Cycle: push it back for repeated use.
            let clone = response.clone();
            queue.push_back(clone);
            response
        } else {
            MockResponse {
                text: "Mock response (no responses configured)".to_owned(),
                tool_calls: vec![],
                input_tokens: 10,
                output_tokens: 10,
            }
        }
    }

    /// Number of calls made so far.
    pub fn calls_made(&self) -> u64 {
        *self.calls_made.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_text_only() {
        let driver = MockLlmDriver::text_only("hello", Duration::from_millis(1));
        let resp = driver.complete().await;
        assert_eq!(resp.text, "hello");
        assert!(resp.tool_calls.is_empty());
    }

    #[tokio::test]
    async fn test_tool_then_text() {
        let driver = MockLlmDriver::single_tool_then_text(
            "shell",
            serde_json::json!({"command": "ls"}),
            "Done!",
        );
        let r1 = driver.complete().await;
        assert_eq!(r1.tool_calls.len(), 1);
        assert_eq!(r1.tool_calls[0].name, "shell");

        let r2 = driver.complete().await;
        assert_eq!(r2.text, "Done!");
    }

    #[tokio::test]
    async fn test_cycling() {
        let driver = MockLlmDriver::text_only("cycle", Duration::from_millis(1));
        let _ = driver.complete().await;
        let _ = driver.complete().await;
        assert_eq!(driver.calls_made(), 2);
    }
}
