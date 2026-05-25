// 0.4.76（product-review B2 step 1）：从 channels/+page.svelte 抽出的 plugin
// builder 示例文本与步骤标签。这些都是只读静态数据，跟具体 channel 状态无关，
// 上提到 _lib 后让 page.svelte 减重 ~75 行。

export const PLUGIN_MANIFEST_EXAMPLE = `{
  "plugin": {
    "preset": { "provider": "openai_compatible" }
  }
}`;

export const PRIVATE_PLUGIN_MANIFEST_EXAMPLE = `{
  "plugin": {
    "request": {
      "chat_path": "/private/chat",
      "headers": { "X-Api-Key": "{{api_key}}" },
      "body": {
        "modelName": "{{model}}",
        "prompt": "{{last_user_message}}",
        "stream": "{{stream}}",
        "limit": "{{max_tokens}}"
      }
    },
    "response": {
      "openai_compatible": false,
      "content_path": "result.text",
      "finish_reason_path": "result.finish",
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output"
      }
    },
    "stream": {
      "openai_compatible": false,
      "event_path": "payload",
      "ignore_events": ["ping"],
      "done_events": ["close"],
      "content_path": "token",
      "tool_calls_path": "tool_calls",
      "finish_reason_path": "finish",
      "done": ["[DONE]", "EOF"],
      "done_path": "type",
      "done_values": ["message_stop"],
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output"
      }
    }
  }
}`;

export const PLUGIN_REPLAY_SAMPLE = `event: token
data: {"payload":{"rid":"r1","model_name":"native","speaker":"assistant"}}

data: {"payload":{"token":"he"}}

data: {"payload":{"token":"llo"}}

data: {"payload":{"finish":"done","usage":{"input":3,"output":2}}}

data: {"payload":{"type":"message_stop"}}
`;

export const RESPONSE_SAMPLE_PLACEHOLDER = `{"result":{"text":"hello"},"usage":{"input":1,"output":2}}`;
export const PROBE_BODY_PLACEHOLDER = `{"model":"{{model}}","messages":[{"role":"user","content":"Hi"}]}`;

export const PLUGIN_BUILDER_STEPS = [
	'Preset',
	'Auth',
	'Request',
	'Response',
	'SSE',
	'Test',
	'Save'
] as const;

export type PluginBuilderStep = (typeof PLUGIN_BUILDER_STEPS)[number];
