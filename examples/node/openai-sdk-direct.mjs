#!/usr/bin/env node
/**
 * OpenAI SDK direct example.
 *
 * Install once:
 *   npm install openai
 *
 * Run:
 *   KOOIX_BASE_URL=http://localhost:8000 \
 *   KOOIX_API_KEY=<project-api-key> \
 *   MODEL=gpt-4o-mini \
 *   node examples/node/openai-sdk-direct.mjs
 */
import OpenAI from 'openai';

const baseURL = process.env.KOOIX_BASE_URL ?? 'http://localhost:8000';
const apiKey = process.env.KOOIX_API_KEY;
const model = process.env.MODEL ?? 'gpt-4o-mini';

if (!apiKey) {
  console.error('KOOIX_API_KEY is required');
  process.exit(1);
}

const client = new OpenAI({
  baseURL: `${baseURL.replace(/\/$/, '')}/v1`,
  apiKey,
});

const completion = await client.chat.completions.create({
  model,
  messages: [
    { role: 'system', content: 'You are a concise gateway smoke tester.' },
    { role: 'user', content: 'Reply with one short sentence.' },
  ],
});

console.log(completion.choices[0]?.message?.content ?? completion);
