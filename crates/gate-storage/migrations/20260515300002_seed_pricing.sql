-- Default global pricing rules for major LLM providers (as of 2025-Q4).
-- channel_id IS NULL = global defaults; override per-channel when needed.
-- All rates in USD.

-- ═══════════════════════════════════════════════════════════════════
-- OpenAI — Token models
-- ═══════════════════════════════════════════════════════════════════

-- gpt-4o
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('gpt-4o', 'input_tokens',        'per_million_tokens', 2.50,  '{}', 'OpenAI gpt-4o input'),
('gpt-4o', 'output_tokens',       'per_million_tokens', 10.00, '{}', 'OpenAI gpt-4o output'),
('gpt-4o', 'cached_input_tokens', 'per_million_tokens', 1.25,  '{}', 'OpenAI gpt-4o cached (50% off)'),
('gpt-4o', 'batch_multiplier',    'multiplier',         0.50,  '{}', 'OpenAI batch 50% off');

-- gpt-4o-mini
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('gpt-4o-mini', 'input_tokens',        'per_million_tokens', 0.15,  '{}', 'OpenAI gpt-4o-mini input'),
('gpt-4o-mini', 'output_tokens',       'per_million_tokens', 0.60,  '{}', 'OpenAI gpt-4o-mini output'),
('gpt-4o-mini', 'cached_input_tokens', 'per_million_tokens', 0.075, '{}', 'OpenAI gpt-4o-mini cached'),
('gpt-4o-mini', 'batch_multiplier',    'multiplier',         0.50,  '{}', 'OpenAI batch 50% off');

-- gpt-4.1
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('gpt-4.1',      'input_tokens',        'per_million_tokens', 2.00, '{}', 'OpenAI gpt-4.1 input'),
('gpt-4.1',      'output_tokens',       'per_million_tokens', 8.00, '{}', 'OpenAI gpt-4.1 output'),
('gpt-4.1',      'cached_input_tokens', 'per_million_tokens', 0.50, '{}', 'OpenAI gpt-4.1 cached (75% off)'),
('gpt-4.1-mini', 'input_tokens',        'per_million_tokens', 0.40, '{}', 'OpenAI gpt-4.1-mini input'),
('gpt-4.1-mini', 'output_tokens',       'per_million_tokens', 1.60, '{}', 'OpenAI gpt-4.1-mini output'),
('gpt-4.1-mini', 'cached_input_tokens', 'per_million_tokens', 0.10, '{}', 'OpenAI gpt-4.1-mini cached'),
('gpt-4.1-nano', 'input_tokens',        'per_million_tokens', 0.10,  '{}', 'OpenAI gpt-4.1-nano input'),
('gpt-4.1-nano', 'output_tokens',       'per_million_tokens', 0.40,  '{}', 'OpenAI gpt-4.1-nano output'),
('gpt-4.1-nano', 'cached_input_tokens', 'per_million_tokens', 0.025, '{}', 'OpenAI gpt-4.1-nano cached');

-- o3 / o4-mini (reasoning)
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('o3',       'input_tokens',        'per_million_tokens', 10.00, '{}', 'OpenAI o3 input'),
('o3',       'output_tokens',       'per_million_tokens', 40.00, '{}', 'OpenAI o3 output (incl. reasoning)'),
('o3',       'cached_input_tokens', 'per_million_tokens', 2.50,  '{}', 'OpenAI o3 cached (75% off)'),
('o4-mini',  'input_tokens',        'per_million_tokens', 1.10,  '{}', 'OpenAI o4-mini input'),
('o4-mini',  'output_tokens',       'per_million_tokens', 4.40,  '{}', 'OpenAI o4-mini output'),
('o4-mini',  'cached_input_tokens', 'per_million_tokens', 0.275, '{}', 'OpenAI o4-mini cached (75% off)');

-- ═══════════════════════════════════════════════════════════════════
-- OpenAI — Image generation
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('dall-e-3', 'per_image', 'per_image', 0.040,  '{"quality":"standard","size":"1024x1024"}',   'DALL-E 3 std 1024²'),
('dall-e-3', 'per_image', 'per_image', 0.080,  '{"quality":"standard","size":"1024x1792"}',   'DALL-E 3 std 1024×1792'),
('dall-e-3', 'per_image', 'per_image', 0.080,  '{"quality":"standard","size":"1792x1024"}',   'DALL-E 3 std 1792×1024'),
('dall-e-3', 'per_image', 'per_image', 0.080,  '{"quality":"hd","size":"1024x1024"}',         'DALL-E 3 HD 1024²'),
('dall-e-3', 'per_image', 'per_image', 0.120,  '{"quality":"hd","size":"1024x1792"}',         'DALL-E 3 HD 1024×1792'),
('dall-e-3', 'per_image', 'per_image', 0.120,  '{"quality":"hd","size":"1792x1024"}',         'DALL-E 3 HD 1792×1024');

-- gpt-image-1 (token-based image gen)
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('gpt-image-1', 'input_tokens',        'per_million_tokens', 5.00,  '{}', 'gpt-image-1 text input'),
('gpt-image-1', 'image_input_tokens',  'per_million_tokens', 10.00, '{}', 'gpt-image-1 image input'),
('gpt-image-1', 'image_output_tokens', 'per_million_tokens', 40.00, '{}', 'gpt-image-1 image output');

-- dall-e-2
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('dall-e-2', 'per_image', 'per_image', 0.020, '{"size":"1024x1024"}', 'DALL-E 2 1024²'),
('dall-e-2', 'per_image', 'per_image', 0.018, '{"size":"512x512"}',   'DALL-E 2 512²'),
('dall-e-2', 'per_image', 'per_image', 0.016, '{"size":"256x256"}',   'DALL-E 2 256²');

-- ═══════════════════════════════════════════════════════════════════
-- OpenAI — Audio
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('tts-1',    'per_character_tts', 'per_million_characters', 15.00, '{}', 'OpenAI TTS standard'),
('tts-1-hd', 'per_character_tts', 'per_million_characters', 30.00, '{}', 'OpenAI TTS HD'),
('whisper-1', 'per_minute_audio', 'per_minute',             0.006, '{}', 'OpenAI Whisper STT per minute');

-- ═══════════════════════════════════════════════════════════════════
-- OpenAI — Embeddings
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('text-embedding-3-small', 'input_tokens', 'per_million_tokens', 0.020, '{}', 'Embedding 3 small'),
('text-embedding-3-large', 'input_tokens', 'per_million_tokens', 0.130, '{}', 'Embedding 3 large'),
('text-embedding-ada-002', 'input_tokens', 'per_million_tokens', 0.100, '{}', 'Embedding ada-002');

-- ═══════════════════════════════════════════════════════════════════
-- Anthropic
-- ═══════════════════════════════════════════════════════════════════

-- Claude Opus 4
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('claude-opus-4-20250514',   'input_tokens',         'per_million_tokens', 15.00,  '{}', 'Claude Opus 4 input'),
('claude-opus-4-20250514',   'output_tokens',        'per_million_tokens', 75.00,  '{}', 'Claude Opus 4 output'),
('claude-opus-4-20250514',   'cached_input_tokens',  'per_million_tokens', 1.50,   '{}', 'Claude Opus 4 cache read (90% off)'),
('claude-opus-4-20250514',   'cache_write_tokens',   'per_million_tokens', 18.75,  '{"cache_ttl":"5m"}', 'Opus 4 cache write 5m (1.25x)'),
('claude-opus-4-20250514',   'cache_write_tokens',   'per_million_tokens', 30.00,  '{"cache_ttl":"1h"}', 'Opus 4 cache write 1h (2x)'),
('claude-opus-4-20250514',   'batch_multiplier',     'multiplier',         0.50,   '{}', 'Anthropic batch 50% off');

-- Claude Sonnet 4
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('claude-sonnet-4-20250514', 'input_tokens',         'per_million_tokens', 3.00,  '{}', 'Claude Sonnet 4 input'),
('claude-sonnet-4-20250514', 'output_tokens',        'per_million_tokens', 15.00, '{}', 'Claude Sonnet 4 output'),
('claude-sonnet-4-20250514', 'cached_input_tokens',  'per_million_tokens', 0.30,  '{}', 'Claude Sonnet 4 cache read'),
('claude-sonnet-4-20250514', 'cache_write_tokens',   'per_million_tokens', 3.75,  '{"cache_ttl":"5m"}', 'Sonnet 4 cache write 5m'),
('claude-sonnet-4-20250514', 'cache_write_tokens',   'per_million_tokens', 6.00,  '{"cache_ttl":"1h"}', 'Sonnet 4 cache write 1h'),
('claude-sonnet-4-20250514', 'batch_multiplier',     'multiplier',         0.50,  '{}', 'Anthropic batch 50% off');

-- Claude Haiku 4
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('claude-haiku-4-20250414',  'input_tokens',         'per_million_tokens', 1.00, '{}', 'Claude Haiku 4 input'),
('claude-haiku-4-20250414',  'output_tokens',        'per_million_tokens', 5.00, '{}', 'Claude Haiku 4 output'),
('claude-haiku-4-20250414',  'cached_input_tokens',  'per_million_tokens', 0.10, '{}', 'Claude Haiku 4 cache read'),
('claude-haiku-4-20250414',  'cache_write_tokens',   'per_million_tokens', 1.25, '{"cache_ttl":"5m"}', 'Haiku 4 cache write 5m'),
('claude-haiku-4-20250414',  'cache_write_tokens',   'per_million_tokens', 2.00, '{"cache_ttl":"1h"}', 'Haiku 4 cache write 1h'),
('claude-haiku-4-20250414',  'batch_multiplier',     'multiplier',         0.50, '{}', 'Anthropic batch 50% off');

-- ═══════════════════════════════════════════════════════════════════
-- Google Gemini
-- ═══════════════════════════════════════════════════════════════════

-- Gemini 2.5 Pro (context tier pricing)
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, priority, description) VALUES
('gemini-2.5-pro', 'input_tokens',  'per_million_tokens', 1.25,  '{}',                       0, 'Gemini 2.5 Pro input ≤200k'),
('gemini-2.5-pro', 'input_tokens',  'per_million_tokens', 2.50,  '{"context_above":200000}',  1, 'Gemini 2.5 Pro input >200k'),
('gemini-2.5-pro', 'output_tokens', 'per_million_tokens', 10.00, '{}',                       0, 'Gemini 2.5 Pro output ≤200k'),
('gemini-2.5-pro', 'output_tokens', 'per_million_tokens', 15.00, '{"context_above":200000}',  1, 'Gemini 2.5 Pro output >200k'),
('gemini-2.5-pro', 'batch_multiplier', 'multiplier',      0.50,  '{}',                       0, 'Google batch 50% off');

-- Gemini 2.5 Flash
INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('gemini-2.5-flash',      'input_tokens',  'per_million_tokens', 0.15, '{}', 'Gemini 2.5 Flash input'),
('gemini-2.5-flash',      'output_tokens', 'per_million_tokens', 0.60, '{}', 'Gemini 2.5 Flash output (non-thinking)'),
('gemini-2.5-flash-lite', 'input_tokens',  'per_million_tokens', 0.10, '{}', 'Gemini 2.5 Flash-Lite input'),
('gemini-2.5-flash-lite', 'output_tokens', 'per_million_tokens', 0.40, '{}', 'Gemini 2.5 Flash-Lite output');

-- ═══════════════════════════════════════════════════════════════════
-- DeepSeek
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('deepseek-chat',   'input_tokens',         'per_million_tokens', 0.27, '{}', 'DeepSeek V3 input (cache miss)'),
('deepseek-chat',   'output_tokens',        'per_million_tokens', 1.10, '{}', 'DeepSeek V3 output'),
('deepseek-chat',   'cached_input_tokens',  'per_million_tokens', 0.07, '{}', 'DeepSeek V3 cache hit (74% off)'),
('deepseek-reasoner', 'input_tokens',       'per_million_tokens', 0.55, '{}', 'DeepSeek R1 input'),
('deepseek-reasoner', 'output_tokens',      'per_million_tokens', 2.19, '{}', 'DeepSeek R1 output'),
('deepseek-reasoner', 'cached_input_tokens','per_million_tokens', 0.14, '{}', 'DeepSeek R1 cache hit');

-- ═══════════════════════════════════════════════════════════════════
-- Moonshot / Kimi (context-tier, CNY converted to USD approx)
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('moonshot-v1-8k',   'input_tokens',  'per_million_tokens', 1.70, '{}', 'Moonshot 8k (~¥12/M)'),
('moonshot-v1-8k',   'output_tokens', 'per_million_tokens', 1.70, '{}', 'Moonshot 8k output'),
('moonshot-v1-32k',  'input_tokens',  'per_million_tokens', 3.40, '{}', 'Moonshot 32k (~¥24/M)'),
('moonshot-v1-32k',  'output_tokens', 'per_million_tokens', 3.40, '{}', 'Moonshot 32k output'),
('moonshot-v1-128k', 'input_tokens',  'per_million_tokens', 17.0, '{}', 'Moonshot 128k (~¥120/M)'),
('moonshot-v1-128k', 'output_tokens', 'per_million_tokens', 17.0, '{}', 'Moonshot 128k output');

-- ═══════════════════════════════════════════════════════════════════
-- Mistral
-- ═══════════════════════════════════════════════════════════════════

INSERT INTO pricing_rules (model, dimension, unit, rate, conditions, description) VALUES
('mistral-large-latest', 'input_tokens',  'per_million_tokens', 2.00, '{}', 'Mistral Large input'),
('mistral-large-latest', 'output_tokens', 'per_million_tokens', 6.00, '{}', 'Mistral Large output'),
('mistral-small-latest', 'input_tokens',  'per_million_tokens', 0.20, '{}', 'Mistral Small input'),
('mistral-small-latest', 'output_tokens', 'per_million_tokens', 0.60, '{}', 'Mistral Small output');
