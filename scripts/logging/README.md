# Logging Helpers

## LLM CLI wrapper (logs stdin/stdout)
```
node scripts/logging/llm-wrapper.js --label gemini -- gemini -m gemini-2.5-pro -o text
```

Logs are written to `logs/llm/<label>-YYYYMMDD-HHMMSS-PID.log`.

## Chat log (manual append)
```
node scripts/logging/chat-log.js "Summary or raw text"
```

Appends to `logs/chat/YYYY-MM-DD.log`.
