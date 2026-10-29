"""Write the tuning and held-out Mermaid blocks as JSON lines for the Rust gate.

usage: python3 -I gate_blocks.py OUT.jsonl TUNE_FILE... -- HELD_FILE...
Each line is {"set": "tune" | "held", "src": BLOCK}. Held-out blocks exclude
every block in the tuning sample, as in corpus2.py. Blocks are read as data only.
"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, '..', 'proto'))
from corpus_extract import blocks_from  # noqa: E402


def main():
    out = sys.argv[1]
    args = sys.argv[2:]
    cut = args.index('--')
    tune = blocks_from(args[:cut])
    seen = set(tune)
    held = [b for b in blocks_from(args[cut + 1:]) if b not in seen]
    with open(out, 'w', encoding='utf-8') as f:
        for which, blocks in (('tune', tune), ('held', held)):
            for b in blocks:
                f.write(json.dumps({'set': which, 'src': b}) + '\n')
    print('tuning blocks', len(tune), 'held-out blocks', len(held))


if __name__ == '__main__':
    main()
