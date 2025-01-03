# foreman

Foreman is a self-hostable Rust-based agent which retrieves jobs from a control server and executes them inside a containerised environment.

## Features

- **💬 Language agnostic**: Jobs are processed in containerised environments.
- **🔐 Secure by default**: Self-hostable behind a NAT gateway, without the need to be exposed publically over the internet.
- **🚀 Fast, efficient and lightweight**: Compiles to a single binary executable

## Installation

Install the Rust toolchain via rustup.

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Compile release build.

```bash
cargo build --release
```

## Usage

The foreman binary expects to find a configuration file named `foreman.toml` in one of the following locations:

- The current working directory
- `$HOME/.foreman/foreman.toml`
- At a path specified by the `FOREMAN_CONFIG` environment variable. e.g. `FOREMAN_CONFIG=/path/to/foreman.toml`

Refer to [example.foreman.toml](example.foreman.toml) for an explanation of the various configuration options and their defaults.

Alternatively, config values can be specified via environment variables.
Each environment variable should be prefixed with `FOREMAN_`.

For example ...

```toml
[core]
port = 8080
```

... can be specified via the environment variable `FOREMAN_CORE_PORT=8080`.

Values set via environment variables will override any values specified in `foreman.toml`.

## Concepts

### Foreman

Foreman (this project) is a self-hostable Rust-based agent which retrieves jobs from a control server and executes them inside a containerised environment.
It is intentionally designed to be run in private subnets behind a NAT gateway, without the need to be exposed to the internet directly.

Foreman is similar in spirit to a CI/CD agent but more generic.

### Control Server

At a high level, a control server is a responsible for the following:

- Serves jobs to foreman agents
- Retrieves job execution statuses from foreman agents

The implementation of a control server is not within the scope of this project, though a reference implementation is included for development purposes.
See the Development section below for more information.

TODO - publish an OpenAPI spec

### Job

A job defines a single task that needs to be executed.
It can be anything from running a script to deploying an application.

### Executor

An executor is responsible for executing jobs on behalf of a foreman agent.

Foreman manages Docker as it's job executor.

## Sequence diagram

The following sequence diagram illustrates the flow of a job execution request between foreman, a control server and an executor.

```mermaid
sequenceDiagram
    participant CS as Control Server
    participant F as Foreman
    participant E as Executor (Docker)
    F->>CS: GET /job
    CS-->>F: JSON
    F->>E: Start container
    E->>F: GET /job
    F-->>E: JSON
    E->>E: Execute job
    E->>F: PUT /job/<job-id>
    F->>CS: PUT /job/<job-id>
    CS-->>F: OK
    F->>E: Stop and remove container
```

## Job schemas

TODO

## Authoring a job processor image

Foreman will create a container based on the `image` defined in a job, pulling the image if necessary.

Containers launched by foreman are expected to query the foreman agent's REST API for their associated job, execute the job and then return a result.

When a container is ready it MUST perform a GET request to the URL contained in the `FOREMAN_GET_JOB_ENDPOINT` environment variable.
This endpoint returns a JSON object containing the job `id` and it's `body`.

Likewise the container MUST perform a PUT request to the URL contained in the `FOREMAN_PUT_JOB_ENDPOINT` environment variable with updates to the job's status.
When sending requests to this endpoint the only requirement is the following headers must be set in the request.

| name                   | required | description                                                                                 |
| ---------------------- | -------- | ------------------------------------------------------------------------------------------- |
| x-foreman-job-status   | YES      | MUST be either 'running' or 'completed'                                                     |
| x-foreman-job-progress | NO       | A floating point number representing the progress of the job. Defaults to 0.0 if undefined. |

Requests sent to this endpoint are forwarded to the job's `callbackUrl` as-is.
The `completed` status is a terminal state and can be set at-most once per job.
It is invalid to send a PUT request with `x-foreman-job-status` set to `running` on a completed job.

A container becomes eligible for removal once it's status changes to `completed`.

## Development

A reference control server is defined in `control_server`.

To run the server, `cd` into the `control_server` directory and run:

```bash
deno run -A index.ts
```

In a separate terminal, start foreman.

```bash
cargo run
```