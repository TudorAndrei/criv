# Find ADR candidates with criv

An ADR records a lasting architectural choice and its reason. Do not create one
for every code change. Ask the agent to use the repository record as evidence
before it writes an ADR.

1. Run `criv init` from the repository root.
2. Read the existing documentation and ADRs.
3. If they are available, read past project conversations for decisions and their reasons.
4. Compare that evidence with the current code.
5. Write ADRs for lasting decisions that the evidence supports.
6. Run `criv watch --once` and `criv check`.
