---
id: pre-commit-managers
kind: doc
title: Pre-commit Manager Integration
---

# Pre-commit Manager Integration

criv does not install Git hooks or change `core.hooksPath`. Add criv to the
hook manager already used by your repository. Run the commit commands in order:

```sh
criv watch --once
criv check --changed
criv enforce --stage commit
```

Add this command to a push hook:

```sh
criv enforce --stage push
```

Keep the commit commands serial. `criv check --changed` reads the state written
by `criv watch --once`.

## hk

```pkl
local criv_check = new Step {
  check = "criv watch --once && criv check --changed"
}

local criv_enforce = new Step {
  check = "criv enforce --stage commit"
}

hooks {
  ["pre-commit"] {
    steps {
      ["criv-check"] = criv_check
      ["criv-enforce"] = criv_enforce
    }
  }

  ["pre-push"] {
    steps {
      ["criv-enforce"] = new Step { check = "criv enforce --stage push" }
    }
  }
}
```

## lefthook

```yaml
pre-commit:
  parallel: false
  commands:
    criv-watch:
      run: criv watch --once
    criv-check:
      run: criv check --changed
    criv-enforce:
      run: criv enforce --stage commit

pre-push:
  commands:
    criv-enforce:
      run: criv enforce --stage push
```

## Husky

Run `npx husky init`, then replace `.husky/pre-commit` with:

```sh
criv watch --once
criv check --changed
criv enforce --stage commit
```

Add `.husky/pre-push`:

```sh
criv enforce --stage push
```

## Python pre-commit

Add repository-local hooks to `.pre-commit-config.yaml`:

```yaml
repos:
  - repo: local
    hooks:
      - id: criv-watch
        name: Refresh criv state
        entry: criv watch --once
        language: system
        pass_filenames: false
        stages: [pre-commit]
      - id: criv-check
        name: Check changed documentation
        entry: criv check --changed
        language: system
        pass_filenames: false
        stages: [pre-commit]
      - id: criv-enforce
        name: Enforce criv policy
        entry: criv enforce --stage commit
        language: system
        pass_filenames: false
        stages: [pre-commit]
      - id: criv-enforce-push
        name: Enforce criv push policy
        entry: criv enforce --stage push
        language: system
        pass_filenames: false
        stages: [pre-push]
```

Then run `pre-commit install --hook-type pre-commit --hook-type pre-push`.

## simple-git-hooks

Add hook commands to `package.json`:

```json
{
  "simple-git-hooks": {
    "pre-commit": "criv watch --once && criv check --changed && criv enforce --stage commit",
    "pre-push": "criv enforce --stage push"
  }
}
```

Run the manager install command after you update `package.json`.

See [[tooling|Tooling and Git Hooks]] for the hook-stage contract.
