# Mermaid: charts and hierarchies

Check each renders, labels don't overlap badly, and colors follow the theme.

## Quadrant chart

```mermaid
quadrantChart
    title Reach and engagement
    x-axis Low reach --> High reach
    y-axis Low engagement --> High engagement
    quadrant-1 Expand
    quadrant-2 Promote
    quadrant-3 Re-evaluate
    quadrant-4 Improve
    Campaign A: [0.3, 0.6]
    Campaign B: [0.45, 0.23]
    Campaign C: [0.57, 0.69]
    Campaign D: [0.78, 0.34]
```

## XY chart

```mermaid
xychart-beta
    title "Sales revenue"
    x-axis [jan, feb, mar, apr, may, jun]
    y-axis "Revenue (k$)" 0 --> 100
    bar [52, 60, 75, 82, 70, 95]
    line [52, 60, 75, 82, 70, 95]
```

## Sankey

```mermaid
sankey-beta
Solar,Grid,60
Wind,Grid,40
Grid,Homes,55
Grid,Industry,35
Grid,Losses,10
```

## Radar

```mermaid
radar-beta
  title Restaurant comparison
  axis food["Food"], service["Service"], price["Price"]
  axis ambiance["Ambiance"]
  curve a["Restaurant A"]{4, 3, 2, 4}
  curve b["Restaurant B"]{3, 4, 3, 3}
  graticule polygon
  max 5
```

## Treemap

```mermaid
treemap-beta
"Budget"
    "Operations"
        "Salaries": 700000
        "Equipment": 200000
    "Marketing"
        "Advertising": 400000
        "Events": 100000
```

## Packet

```mermaid
packet-beta
0-15: "Source Port"
16-31: "Destination Port"
32-63: "Sequence Number"
64-95: "Acknowledgment Number"
96-99: "Data Offset"
100-105: "Reserved"
106: "URG"
107: "ACK"
108-127: "Window"
```

## Timeline

```mermaid
timeline
    title History of the editor
    2023 : Fork created
    2024 : Trimmed panels
         : Custom keymap
    2025 : Own markdown renderer
    2026 : Owned Mermaid crate
```

## Mindmap

```mermaid
mindmap
  root((Editor))
    Markdown
      Preview
      HTML subset
    Diagrams
      Flowchart
      Sequence
    Git
      Blame
      Diff
```

## Kanban

```mermaid
kanban
  todo[Todo]
    t1[Write spec]
    t2[Smoke test preview]@{ priority: 'High' }
  doing[In progress]
    t3[HTML lexer]@{ assigned: 'neo' }
  done[Done]
    t4[Mermaid swap]
```

## Tree view

```mermaid
treeView-beta
    src/
        main.rs
        lib.rs
    Cargo.toml
    README.md
```
