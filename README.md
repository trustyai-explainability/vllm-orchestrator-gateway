# vllm-orchestrator-gateway

This gateway service enables controlled interaction with the FMS Guardrails Orchestrator by enforcing stricter access to its exposed endpoints. It provides a mechanism of configuring fixed detector pipelines, and then provides a unique /v1/chat/completions endpoint per configured detector pipeline. This allows for drop-in replacement of an unguardrailed chat completions model with a guardrailed one.

### Getting started
To see the entire stack of the vllm-orchestrator-gateway there are a few services that need to be spun up. Some of these can be swapped for other services that use follow certain api's, such as the detectors and generation models.

- [FMS Guardrails Orchestrator](https://github.com/foundation-model-stack/fms-guardrails-orchestrator)
- [Guardrails Regex Detector](https://github.com/trustyai-explainability/guardrails-regex-detector)
- [VLLM using Qwen/Qwen2.5-1.5B-Instruct](https://docs.vllm.ai/en/latest/getting_started/quickstart.html#openai-compatible-server)


### Sample config
The config has 3 main fields, `orchestrator`, `detectors` and `routes`.

`orchestrator` is where the `orchestrator` service lives.

`detectors` are detectors services that have been defined in the `fms-guardrails-orchestrator` config file. You can specify what detector belongs to input and/or output.

`routes` are the dynamically exposed routes used to enforce detectors onto endpoints such as the `pii` endpoint that registers our `regex-language` detector. You can also specify no detectors such as the `passthrough` route down below.

`fallback_message` in the `routes` field is used as a response from the gateway when a detection is found either in the input or output.

```yaml
orchestrator:
  host: localhost
  port: 8085
detectors:
  - name: regex-language
    input: false
    output: true
    detector_params:
      regex:
        - email
        - ssn
routes:
  - name: pii
    detectors:
      - regex-language
    fallback_message: "I'm sorry, I'm afraid I can't do that."
  - name: passthrough
    detectors:
```


### Sample request
```bash
curl "localhost:8090/pii/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{
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
```
### Sample response with generation
```bash
{
  "choices": [
    {
      "finish_reason": "stop",
      "index": 0,
      "logprobs": null,
      "message": {
        "audio": null,
        "content": "Hello! It looks like you've provided my email address and a social security number. I'm just an AI assistant and not an email or social security system. Please correct this.",
        "refusal": null,
        "role": "assistant",
        "tool_calls": null
      }
    }
  ],
  "created": 1741182909,
  "detections": null,
  "id": "chatcmpl-971213a0e09446a8b11bd447db0f3a64",
  "model": "Qwen/Qwen2.5-1.5B-Instruct",
  "object": "chat.completion",
  "service_tier": null,
  "system_fingerprint": null,
  "usage": {
    "completion_tokens": 37,
    "prompt_tokens": 61,
    "total_tokens": 98
  },
  "warnings": null
}
```

### Sample response with found detection
```bash
{                                                              
  "choices": [   
    {                  
      "finish_reason": "stop",
      "index": 0,     
      "logprobs": null,                                        
      "message": {                                             
        "audio": null,                                         
        "content": "I'm sorry, I'm afraid I can't do that.",
        "refusal": null,
        "role": "assistant", 
        "tool_calls": null
      }                     
    }                   
  ],                   
  "created": 1741182848,
  "detections": {
    "input": null,        
    "output": [                                                
      {                                                        
        "choice_index": 0,                                     
        "results": [                                           
          {                                                    
            "detection": "EmailAddress",                                                                                      
            "detection_type": "pii",
            "detector_id": "regex-language",
            "end": 176,                                        
            "score": 1.0,                                      
            "start": 152,                                      
            "text": "someemail@somedomain.com"
          }                                                    
        ]                                                      
      }                                                        
    ]                                                          
  },                                                           
  "id": "16a0abbf4b0c431e885be5cfa4ff1c4b",
  "model": "Qwen/Qwen2.5-1.5B-Instruct",
  "object": "chat.completion",                                 
  "service_tier": null,                                        
  "system_fingerprint": null,                                  
  "usage": {                                                   
    "completion_tokens": 83,                                   
    "prompt_tokens": 61,                                       
    "total_tokens": 144                                        
  },                                                           
  "warnings": [                                                
    {                                                          
      "message": "Unsuitable output detected.",
      "type": "UNSUITABLE_OUTPUT"
    }                                                          
  ]                                                            
}             
```

