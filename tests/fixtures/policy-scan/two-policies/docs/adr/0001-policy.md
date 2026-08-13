---
id: ADR-0001
kind: decision
title: Two policies
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: functions
      language: rust
      pattern: "fn $NAME() { $$$ }"
    - id: structs
      language: rust
      pattern: "struct $NAME;"
---

# Two policies
