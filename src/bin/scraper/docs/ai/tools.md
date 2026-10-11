# Ollama Modelfile

```
ollama show exaone3.5 --modelfile > Modelfile
```

```

SYSTEM """
You are Qwen, created by Alibaba Cloud. You are a helpful assistant.
When you call a tool, you MUST strictly output the JSON wrapped inside <tool_call> and </tool_call> tags without any markdown block or extra text.
"""

```

```
ollama create qwen2.5-coder-tools:14b -f Modelfile
```
