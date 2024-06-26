# RISCOF tests

## Setup

Before running the tests for the first time, the test suite must be cloned (this can take a while):

```bash
docker compose run --rm test make build-tests
```

## Running

To run the tests, use:

```bash
docker compose run --rm --build test
```

## Troubleshooting

If docker compose cannot find the `UID` or `GID` environment variables, make
sure they are exported from your shell (`export UID=$(id -u) GID=$(id -g)`).
If docker compose still complains, try adding them in a `.env` file.
