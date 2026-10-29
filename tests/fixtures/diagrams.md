# Diagrams

A top-down flowchart with a decision:

```mermaid
flowchart TD
    A[Receive order] --> B{In stock?}
    B -->|yes| C[Ship it]
    B -->|no| D[Back-order]
    D -.-> E[(Orders DB)]
    C --> F([Done])
```

A left-to-right pipeline that may need to turn on narrow screens:

```mermaid
graph LR
    fetch[Fetch sources] --> parse[Parse] --> check[Type check] --> build[Build] --> test[Run tests] --> ship[Publish]
```

Shapes, heads and a title:

```mermaid
---
title: Shapes
---
flowchart TD
    a[Rectangle] --> b(Rounded)
    b --> c([Stadium])
    c --> d[[Subroutine]]
    d --o e((Circle))
    e --x f{{Hexagon}}
    f ==> g>Flag]
    g --- h[/Lean/]
```

A cycle and a self-loop:

```mermaid
flowchart TD
    idle --> running
    running --> idle
    running --> running
```

Labels in other scripts:

```{mermaid}
:caption: MyST options are ignored
flowchart LR
    u[ユーザー] -->|リクエスト| s[認証サービス]
    s --> d[(데이터베이스)]
```

Subgraphs are listed as an outline for now:

```mermaid
flowchart TB
    subgraph Frontend
        ui[Web UI] --> api
    end
    subgraph Backend
        api[API] --> db[(Postgres)]
    end
```

A sequence diagram:

```mermaid
sequenceDiagram
    autonumber
    participant C as Client
    participant S as Server
    C->>S: GET /items
    alt cached
        S-->>C: 304 Not Modified
    else fresh
        S->>S: load items
        S-->>C: 200 OK
    end
    Note right of S: logs every request
```

Types folio does not draw, and mistakes, show their source:

```mermaid
pie title Pets
    "Dogs" : 386
    "Cats" : 85
```

```mermaid
flowchart TD
    A --> B -|oops
```

```mermaid
```
