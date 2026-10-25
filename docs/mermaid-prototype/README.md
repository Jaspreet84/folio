# Mermaid prototype

A throwaway Python prototype of the diagram layout described in
[../mermaid-plan.md](../mermaid-plan.md). It exists to test the algorithms on
real diagrams before writing them in Rust, and milestone M2 of the plan ports
it. It is research code: not tidy, not part of folio's build, and not tested
beyond the measurements the plan reports.

## What's here

| Path | What it is |
|---|---|
| `proto/proto.py` | The first prototype: parsers, layout, routing and drawing for flowcharts and sequence diagrams (about 1,900 lines). |
| `proto2/make_proto2.py` | Builds `proto2/proto.py` from `proto/proto.py`, adding the correctness fixes from the plan's second revision: cell owners, the port rule, the crossing rule, hard-conflict counting, the LR self-loop flag and CJK line breaks. |
| `proto2/proto.py` | The fixed prototype that the plan's numbers and mock-ups come from. |
| `proto2/check.py` | The checks: text over lines, cell checks, the connectivity trace and the sequence junction check. |
| `proto2/ladder.py` | The three ladder orders (A, B, C) compared in section 7.1. |
| `proto2/corpus2.py`, `analyze2.py` | The run over the GitHub samples and its analysis (`analysis.txt`). |
| `proto2/timing2.py`, `perfile.py`, `contrast.py`, `table2.py`, `mock2.py`, `demo.py` | The other measurements; each writes the `.txt` file of the same name. |
| `proto/mock/`, `proto/t/`, `review/` | Small test diagrams written for this work. |
| `proto/corpus_extract.py` | Pulls Mermaid blocks out of Sourcegraph streaming-search results. |
| `merman-probe/` | The Rust program used to measure merman-ascii 0.9.2 (rejected in section 5.2). |

## Running it

Python 3.12, no packages needed. From this folder:

```sh
python3 -I proto2/proto.py 80 < proto2/flow1.mmd     # mock-up 1
python3 -I proto2/proto.py 80 < proto2/seq1.mmd      # mock-up 2
W=40 proto/run.sh proto/t/*.mmd                      # first prototype, 40 columns
python3 -I proto2/table2.py proto2/*.mmd proto/t/*.mmd
```

## The GitHub samples

The corpus runs use about 24,000 Mermaid blocks collected from public
repositories on GitHub through Sourcegraph search: a 17,450-block tuning
sample and a 6,496-block held-out sample. Those blocks belong to their
authors, so they are not in this repository. `.gitignore` keeps them out.

To repeat a corpus run, put the sample files in `sgdata/` here, then:

```sh
python3 -I proto2/corpus2.py proto2/results.jsonl \
  sgdata/sample_stream_big.txt sgdata/sample_stream.txt -- \
  sgdata/sample_readme.txt sgdata/sample_adr.txt sgdata/selrepo.txt sgdata/sg_test.txt
python3 -I proto2/analyze2.py proto2/results.jsonl > proto2/analysis.txt
```

The first two files are the tuning sample; the files after `--` are the
held-out sample. The plan's go/no-go gates use the held-out sample only, so
don't tune against it.
