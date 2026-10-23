# Mermaid: architecture and modelling

Check each renders; icons that need an icon pack may show as plain boxes.

## Architecture

```mermaid
architecture-beta
    group api(cloud)[API]
    service db(database)[Database] in api
    service disk(disk)[Storage] in api
    service server(server)[Server] in api
    service gateway(internet)[Gateway]
    db:L -- R:server
    disk:T -- B:server
    gateway:R -- L:server
```

## Block

```mermaid
block
  columns 3
  Start(("Start")) space:2
  down<[" "]>(down) space:2
  Decision{{"Decide"}} right<["Yes"]>(right) A["Process A"]
  no<["No"]>(down) space r3<["Done"]>(down)
  B["Process B"] r2<["Done"]>(right) End(("End"))
```

## C4 context

```mermaid
C4Context
    title System context for the editor
    Person(user, "Developer", "Edits code")
    System(editor, "Editor", "Zed fork")
    System_Ext(git, "Git host", "Stores repositories")
    Rel(user, editor, "Uses")
    Rel(editor, git, "Pushes and pulls", "HTTPS")
```

## Requirement

```mermaid
requirementDiagram
  requirement preview_req {
    id: 1
    text: Markdown preview renders diagrams.
    risk: medium
    verifymethod: test
  }
  functionalRequirement html_req {
    id: 1.1
    text: HTML subset renders in the preview.
    risk: low
    verifymethod: inspection
  }
  element markdown_preview {
    type: crate
  }
  preview_req - contains -> html_req
  markdown_preview - satisfies -> html_req
```

## Use case

```mermaid
usecase-beta
actor Developer
Preview("Open preview")
systemBoundary Editor
  Edit[Edit markdown]
end
Developer --> Preview
Developer --> Edit
```

## Swimlane

```mermaid
swimlane-beta LR
  subgraph Author
    write[Write doc]
    fix[Fix comments]
  end
  subgraph Reviewer
    read[Review]
    approve[Approve]
  end
  write --> read
  read -->|changes| fix
  fix --> read
  read --> approve
```

## Event modeling

```mermaid
eventmodeling

timeframe 01 ui CartUI
timeframe 02 command AddItem [[AddItem01]]
timeframe 03 event ItemAdded [[ItemAdded]]

data AddItem01 {
  description: 'john'
  price: 20.4
}

data ItemAdded {
  description: string
  price: number
}
```

## Agent flow

```mermaid
agentflow-beta TB
  flow reviewer["Review Agent"]
    changes["Gather changes"]@{ shape: input }
    analyse["Analyse diff"]@{ shape: task }
    lint["run_linter"]@{ shape: tool }
    ok["Clean?"]@{ shape: decision }

    changes --> analyse --> lint --> ok
  end
```
