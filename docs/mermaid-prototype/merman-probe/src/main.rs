use merman::ascii::{AsciiLayoutProfile, AsciiRenderOptions, AsciiViewportPolicy, OverflowPolicy};
use merman::{AsciiRequest, OperationControl, RenderOutput, RenderRequest, Renderer};
use std::time::Instant;

const SAMPLES: &[(&str, &str)] = &[
("flow_td_loop", "flowchart TD
    A[Receive order] --> B{In stock?}
    B -->|yes| C[Ship it]
    B -->|no| D[Back-order]
    C --> F([Done])
    D -.-> E[(Orders DB)]
    D -->|retry| B
"),
("flow_lr7", "flowchart LR
    A[Lint] --> B[Test] --> C[Build] --> D[Sign] --> E[Publish] --> F[Announce] --> G[Deploy]
"),
("flow_lr7_as_td", "flowchart TD
    A[Lint] --> B[Test] --> C[Build] --> D[Sign] --> E[Publish] --> F[Announce] --> G[Deploy]
"),
("flow_3branch", "flowchart TD
    A[Incoming request] --> B{Route by type}
    B -->|read| C[Cache lookup]
    B -->|write| D[Primary database]
    B -->|admin| E[Audit and approve]
"),
("flow_loops2", "flowchart TD
    A[Draft] --> B[Review]
    B -->|changes| A
    B --> C[Test]
    C -->|fail| A
    C --> D[Ship]
"),
("flow_cjk", "flowchart TD
    A[开始] --> B{检查库存}
    B -->|有| C[发货]
    B -->|无| D[补货]
"),
("loop_simple", "flowchart TD
    A[Start] --> B[Check]
    B --> A
"),
("loop_labeled", "flowchart TD
    A[Start] --> B[Check]
    B -->|retry| A
    B --> C[Done]
"),
("loop_nolabel3", "flowchart TD
    A[Draft] --> B[Review]
    B --> C[Ship]
    B --> A
"),
("flow_lr_loop", "flowchart LR
    A[Draft] --> B[Review]
    B --> C[Ship]
    B -->|changes| A
"),
("td_noretry", "flowchart TD
    A[Receive order] --> B{In stock?}
    B -->|yes| C[Ship it]
    B -->|no| D[Back-order]
    C --> F([Done])
    D -.-> E[(Orders DB)]
"),
("td_retry_plain", "flowchart TD
    A[Receive order] --> B{In stock?}
    B -->|yes| C[Ship it]
    B -->|no| D[Back-order]
    D --> B
"),
("td_retry_rect", "flowchart TD
    A[Receive order] --> B[In stock?]
    B --> C[Ship it]
    B --> D[Back-order]
    D --> B
"),
("seq_alt", "sequenceDiagram
    participant U as User
    participant W as Web app
    participant DB as Database
    U->>W: Log in
    W->>DB: Find user
    DB-->>W: User row
    W->>W: Check hash
    alt valid
        W-->>U: Session cookie
    else invalid
        W-->>U: 401
    end
"),
];

fn main() {
    let r = Renderer::new().with_parse_options(merman::ParseOptions::strict());
    for (name, src) in SAMPLES {
        for (pname, prof) in [("canonical", AsciiLayoutProfile::Canonical), ("compact", AsciiLayoutProfile::Compact)] {
            let t = Instant::now();
            let opts = if std::env::var("PADY0").is_ok() { AsciiRenderOptions::unicode().with_node_padding_y(0).with_layout_profile(prof) } else { AsciiRenderOptions::unicode().with_layout_profile(prof) };
            let res = r.render(RenderRequest::ascii(src, OperationControl::new(), AsciiRequest {
                options: opts,
                viewport: AsciiViewportPolicy::unrestricted().overflow(OverflowPolicy::Allow),
                ..Default::default()
            }));
            let dt = t.elapsed();
            match res {
                Ok(RenderOutput::Ascii(Some(rep))) => {
                    let w = rep.text.lines().map(|l| l.trim_end().chars().count()).max().unwrap_or(0);
                    println!("=== {name} {pname} {:?} width~{w} rows={}", dt, rep.text.lines().count());
                    for l in rep.text.lines() { println!("{}", l.trim_end()); }
                }
                Ok(_) => println!("=== {name} {pname}: no diagram"),
                Err(e) => println!("=== {name} {pname}: error {e}"),
            }
        }
    }
}
