import http from 'k6/http';
import { check, sleep } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';
import { SharedArray } from 'k6/data';

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const API_URL = __ENV.API_URL || 'http://localhost:8080';
const API_KEY = __ENV.API_KEY || 'sk-kg-test-key';

// Custom metrics
const errorRate  = new Rate('errors');
const chatLatency = new Trend('chat_latency', true);
const streamReqs  = new Counter('stream_requests');
const nonStreamReqs = new Counter('non_stream_requests');

// ---------------------------------------------------------------------------
// Realistic message payloads — varying lengths + stream/non-stream mix
// ---------------------------------------------------------------------------

const SHORT_MESSAGES = [
  'Hello!',
  'What is 2+2?',
  'Summarize in one sentence: the sky is blue.',
  'Translate "good morning" to French.',
  'Name three fruits.',
];

const MEDIUM_MESSAGES = [
  'Explain the difference between TCP and UDP in about 100 words. Cover reliability, ordering, and common use cases for each protocol.',
  'Write a short product description for a wireless noise-cancelling headphone. Mention battery life, comfort, and sound quality.',
  'What are the key differences between REST and GraphQL APIs? Include pros and cons of each approach in a concise summary.',
  'Describe the SOLID principles in object-oriented programming. Give a brief example for each principle.',
];

const LONG_MESSAGES = [
  'Write a detailed tutorial on implementing a rate limiter in a web application. Cover token bucket and sliding window algorithms. Include pseudocode, discuss trade-offs between accuracy and memory usage, explain distributed rate limiting with Redis, and provide guidance on choosing appropriate limits for different API endpoints. The tutorial should be comprehensive enough for a mid-level developer to implement. Also discuss how to handle burst traffic and graceful degradation strategies when the rate limiter itself becomes a bottleneck.',
  'Explain the CAP theorem in distributed systems. Provide real-world examples of systems that choose CP vs AP. Discuss how modern databases like CockroachDB, Cassandra, and DynamoDB handle the trade-offs. Include a section on PACELC theorem as an extension. Discuss practical implications for system design, including how to handle network partitions gracefully, strategies for eventual consistency, and conflict resolution approaches like CRDTs and vector clocks.',
];

const CODE_MESSAGES = [
  'Write a Rust function that implements binary search on a sorted slice. Include generics, proper error handling with Result type, and comprehensive unit tests. Add documentation comments explaining the algorithm complexity.',
  'Implement a simple LRU cache in Python with O(1) get and put operations. Use OrderedDict or implement with a doubly-linked list and hash map. Include type hints, docstrings, and pytest test cases.',
];

// Pre-computed weighted selection: 40% short, 35% medium, 20% long, 5% code
const MESSAGE_POOL = new SharedArray('messages', function() {
  const pool = [];
  SHORT_MESSAGES.forEach(m => { for (let i = 0; i < 8; i++) pool.push({ content: m, type: 'short' }); });
  MEDIUM_MESSAGES.forEach(m => { for (let i = 0; i < 9; i++) pool.push({ content: m, type: 'medium' }); });
  LONG_MESSAGES.forEach(m => { for (let i = 0; i < 10; i++) pool.push({ content: m, type: 'long' }); });
  CODE_MESSAGES.forEach(m => { for (let i = 0; i < 3; i++) pool.push({ content: m, type: 'code' }); });
  return pool;
});

const MODELS = ['gpt-4o', 'gpt-4o-mini', 'gpt-4o'];

// ---------------------------------------------------------------------------
// Scenarios
// ---------------------------------------------------------------------------

export const options = {
  scenarios: {
    // Phase 1: gradual ramp-up to find breaking point
    ramp_up: {
      executor: 'ramping-vus',
      startVUs: 10,
      stages: [
        { duration: '30s', target: 100 },
        { duration: '1m',  target: 500 },
        { duration: '2m',  target: 1000 },
        { duration: '1m',  target: 500 },
        { duration: '30s', target: 0 },
      ],
      gracefulRampDown: '10s',
    },
    // Phase 2: constant high load — target 50k rpm (≈833 req/s)
    constant_high: {
      executor: 'constant-arrival-rate',
      rate: 833,
      timeUnit: '1s',
      duration: '2m',
      preAllocatedVUs: 1000,
      maxVUs: 2000,
      startTime: '5m30s',  // starts after ramp_up completes
    },
  },
  thresholds: {
    'http_req_duration': ['p(95)<500', 'p(99)<1000'],
    'errors':           ['rate<0.01'],
    'chat_latency':     ['p(50)<200', 'p(95)<500', 'p(99)<1000'],
  },
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function pickRandom(arr) {
  return arr[Math.floor(Math.random() * arr.length)];
}

function buildPayload() {
  const msg = pickRandom(MESSAGE_POOL);
  const model = pickRandom(MODELS);
  // ~20% of requests are streaming
  const stream = Math.random() < 0.2;

  return {
    body: JSON.stringify({
      model: model,
      messages: [
        { role: 'system', content: 'You are a helpful assistant.' },
        { role: 'user',   content: msg.content },
      ],
      stream: stream,
      max_tokens: 256,
    }),
    stream: stream,
  };
}

// ---------------------------------------------------------------------------
// Main VU function
// ---------------------------------------------------------------------------

export default function () {
  const { body, stream } = buildPayload();

  const params = {
    headers: {
      'Content-Type':  'application/json',
      'Authorization': `Bearer ${API_KEY}`,
    },
    timeout: '10s',
    tags: { stream: String(stream) },
  };

  const res = http.post(`${API_URL}/v1/chat/completions`, body, params);

  chatLatency.add(res.timings.duration);

  if (stream) {
    streamReqs.add(1);
    check(res, {
      'stream: status 200': (r) => r.status === 200,
      'stream: has SSE data': (r) => r.body && r.body.includes('data:'),
    }) || errorRate.add(1);
  } else {
    nonStreamReqs.add(1);
    check(res, {
      'non-stream: status 200': (r) => r.status === 200,
      'non-stream: has choices': (r) => {
        try {
          return JSON.parse(r.body).choices !== undefined;
        } catch (e) {
          return false;
        }
      },
      'non-stream: has usage': (r) => {
        try {
          const u = JSON.parse(r.body).usage;
          return u && u.total_tokens > 0;
        } catch (e) {
          return false;
        }
      },
    }) || errorRate.add(1);
  }
}

// ---------------------------------------------------------------------------
// Summary reporter
// ---------------------------------------------------------------------------

export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    thresholds: data.root_group ? data.root_group.thresholds : {},
    metrics: {},
  };

  const keys = [
    'http_req_duration', 'http_reqs', 'errors',
    'chat_latency', 'stream_requests', 'non_stream_requests',
    'iterations', 'vus', 'vus_max',
  ];

  for (const k of keys) {
    if (data.metrics && data.metrics[k]) {
      summary.metrics[k] = data.metrics[k].values;
    }
  }

  return {
    stdout: textSummary(data, { indent: ' ', enableColors: true }),
    'bench/results/summary.json': JSON.stringify(summary, null, 2),
  };
}

// k6 built-in text summary helper
import { textSummary } from 'https://jslib.k6.io/k6-summary/0.1.0/index.js';
