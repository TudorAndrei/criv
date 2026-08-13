---
id: ADR-0001
kind: decision
title: Duplicate policies
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: duplicate
      language: rust
      pattern: "fn $NAME() { $$$ }"
    - id: duplicate
      language: rust
      pattern: "struct $NAME;"
---

# Duplicate policies
