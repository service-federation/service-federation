# Scripts

Scripts are commands that can depend on services or other scripts. They're defined in the `scripts` section of your config and run with `fed run <name>` or the shorthand `fed <name>`.

## Defining Scripts

```yaml
scripts:
  db:migrate:
    depends_on: [database]
    script: npx prisma db push

  test:integration:
    depends_on: [db:migrate, api]
    script: npm run test:e2e -- "$@"  # "$@" passes arguments from CLI
```

## Running Scripts

```bash
fed run db:migrate                    # Run a script
fed db:migrate                        # Shorthand (if no command collision)
fed test:integration -- -t "auth"     # Pass arguments after --
```

Services started as script dependencies keep running after the script finishes. Use `fed stop` to stop them.

## Isolated Scripts

For integration tests that need a throwaway stack without interfering with your running dev services:

```yaml
scripts:
  test:integration:
    isolated: true     # Fresh ports, scoped volumes, separate containers
    depends_on: [database, api]
    script: npm run test:e2e
```

```bash
fed start                # Dev stack stays running
fed test:integration     # Tests get their own stack, cleaned up after
```

When `isolated: true` is set:

- **Fresh ports** are allocated, independent of your dev stack.
- **Docker containers and volumes** are scoped with a unique isolation ID — no collisions with your running services.
- **Cleanup happens automatically** when the script finishes — containers are stopped and removed.

This is the recommended way to run integration tests. Your dev stack is untouched, and each test run gets a clean environment.

## Environment Variables

Scripts inherit the resolved parameters from the config. You can also set script-specific environment variables:

```yaml
scripts:
  test:integration:
    depends_on: [database]
    environment:
      TEST_MODE: "true"
    script: npm run test:e2e
```

## Script Dependencies

Scripts can depend on both services and other scripts:

```yaml
scripts:
  db:migrate:
    depends_on: [database]
    script: npx prisma migrate deploy

  db:seed:
    depends_on: [db:migrate]
    script: npx prisma db seed

  test:e2e:
    depends_on: [db:seed, api]
    script: npm run test:e2e
```

Dependencies are started in order. Service dependencies are started and health-checked before the script runs.
