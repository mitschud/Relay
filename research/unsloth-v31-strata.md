# Unsloth Qwen3.8 Flash Next v3.1 and Strata compatibility

Checked4 November2026. Research only: existing model/profile,200000-token context,
FP8 PLE and1GiB VRAM reserve were not changed. No complete model was downloaded;
temporary header prefixes were removed. The small JSON evidence file is retained.

Unsloth dates the update4 November and claims15% higher benchmark accuracy than
other frontier quants. This is not a15% increase in numerical precision, a promise
for every quant/task, or a measured gain over this laptop's ISTA IQ3_S setup.
[Unsloth model card](https://huggingface.co/unsloth/Qwen3.8-Flash-Next-GGUF).

## Checks against the installed version

Installed Control/engine0.1.56, engine source build06698c674673. Hugging Face
revision inspected: `d96201194f103c906994f283f504209dc8d2a10c`. Read the headers of all four shards for
UD-IQ3_XXS, UD-Q3_K_XL and UD-Q4_K_XL using installed `tools/gguf_reader.py`.
All passed the installed packer's unmodified `check_split` function: correct
architecture metadata, shard numbering/counts and1224 total tensors.

Verified every core layer's gate/up expert types match. Inspected expert formats
against the engine build's CUDA dispatch tables, including IQ4_NL/Q2_0 downs,
Q5_0 downs and Q4_K/Q5_K with Q5_1/Q8_0 downs. The24 BF16 dense tensors in each
set are QSA indexer projections, already declared BF16 in the installed packer's
FORM table; they are not unexpected BF16 attention-gate rows.

No format/schema blocker was found in these inspected paths. That is not a full
runtime validation: packing reads/converts real tensor data, and no new pack or
live generation was attempted. Build a new pack for each exact quant/revision;
ordinary Unsloth quants use the documented `--compat-bf16` conversion path. Never
reuse the existing ISTA pack with different offsets/weights.
[Strata manual packing guide](https://github.com/Niko1221/Strata/blob/main/docs/UNSLOTH_Q4.md).

Expert payloads from the actual headers, excluding dense weights/KV/runtime:

| Quant | Expert bytes | Approx.GiB |
|---|---:|---:|
| UD-IQ3_XXS |50292326400|46.84|
| UD-Q3_K_XL |56177459200|52.32|
| UD-Q4_K_XL |77017907200|71.73|

These are not whole-system RAM requirements. On64GiB RAM, Q4 deserves particular
caution: the experts alone exceed host RAM and would need paging/a different
residency strategy. IQ3_XXS is the more plausible first v3.1 candidate to measure.

## Independent engram/PLE table

Read BF16, Q8_0 and Q5_1 standalone headers: all expose
`per_layer_token_embd.weight`, shape160x320001536, with the declared encoding.
The pinned engine's `PleReader` looks up this name, validates the shape, and has
BF16/Q8_0/Q5_1 format readers. `--ple-gguf` selects a separate table. This permits
testing a table upgrade with the existing core quant; it does not require updating
the whole model. The BF16 table path is a CPU/SSD lookup/dequantization path, not
a reason to enable a new GPU kernel hack.
[Strata table reader at the installed build](https://github.com/Niko1221/Strata/blob/06698c674673/src/kernels/ngram.cpp).

| Table | Approx.disk GB (decimal) |
|---|---:|
| IQ4_NL |28.8|
| Q5_1 |38.4|
| Current FP8 encoding |51.2|
| Q8_0 |54.4|
| BF16 |102.4|

With Strata's direct SSD reader, the entire table does not need to fit RAM/VRAM.
BF16 increases storage and row payload; actual latency needs measurement because
aligned reads and caching matter. Strata's limited table tests report Q8_0 row
error0.53%, FP8 row error2.64%, and perplexity within1% of BF16 for the tested
quantized tables. These are not coding-agent success rates.
[Strata measurements](https://github.com/Niko1221/Strata/blob/main/docs/DETAILS.md).

Recommendation: keep the working profile, try a separate Q8_0 PLE file/profile
first if a quality comparison is desired. BF16 is a useful reference experiment,
not an established worthwhile upgrade for this laptop. If later adopting v3.1,
validate the exact new pack at short context before reproducing200K settings.
Unsloth's shard-replacement instructions require stamping split metadata for its
llama.cpp layout; do not apply that script to the current working table. A Strata
separate-table override is a distinct integration path.
[Unsloth engram instructions](https://huggingface.co/unsloth/Qwen3.8-Flash-Next-GGUF/blob/main/engram/README.md).
