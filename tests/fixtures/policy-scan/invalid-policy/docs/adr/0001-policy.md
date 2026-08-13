---
id: ADR-0001
kind: decision
title: Invalid policy
status: accepted
governs:
  - src/**
policy:
  patterns:
    - id: invalid
      language: rust
      rule: "not: [valid"
---

# Invalid policy
