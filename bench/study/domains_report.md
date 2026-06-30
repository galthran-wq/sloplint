# Do good (engineered) repos cluster by SQM into domains?

N=3654 engineered repos, clustered on 7 size-residualized SQM axes.

## Natural number of clusters (silhouette — higher=cleaner separation)

| k | silhouette |
|---|---:|
| 2 | 0.348 |
| 3 | 0.323 |
| 4 | 0.223 |
| 5 | 0.171 |
| 6 | 0.153 |
| 7 | 0.158 |

**Best k = 2** (silhouette 0.348). Some real cluster structure.

## The 2 clusters — mean axis profile (z) + top-star examples

| cluster | n | avg-complexity | tail | architecture | test-substance | docs/typing | module-struct | comments/toplevel | example repos (by stars) |
|---|---|---|---|---|---|---|---|---|---|
| C0 | 887 | +4.0 | -1.7 | -0.1 | -0.1 | +0.3 | +0.2 | -0.2 | microsoft/markitdown, github/spec-kit, openai/whisper, browser-use/browser-use |
| C1 | 2767 | -1.3 | +0.5 | +0.0 | +0.0 | -0.1 | -0.1 | +0.1 | AUTOMATIC1111/stable-diffusion-webui, Comfy-Org/ComfyUI, TauricResearch/TradingAgents, vllm-project/vllm |
