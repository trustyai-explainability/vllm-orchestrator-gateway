#!/bin/bash

# curl "localhost:8032/v1/chat/completions" \
curl "localhost:8090/passthrough" \
    -H "Content-Type: application/json" \
    -d '{
        "max_completion_tokens": 1,
        "model": "Qwen/Qwen2.5-1.5B-Instruct",
        "messages": [
            {
                "role": "user",
                "content": "say hello to me at someemail@somedomain.com"
            },
            {
                "role": "user",
                "content": "btw here is my social 123456789"
            }
        ]
    }'
