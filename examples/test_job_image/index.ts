import { Job } from "./job.ts";

const FOREMAN_GET_JOB_ENDPOINT: string = Deno.env.get(
    "FOREMAN_GET_JOB_ENDPOINT",
)!;
const FOREMAN_PUT_JOB_ENDPOINT: string = Deno.env.get(
    "FOREMAN_PUT_JOB_ENDPOINT",
)!;

interface MathJob extends Job {
    body: {
        values: number[];
    };
}

const main = async () => {
    // Fetch the job from Foreman using the GET endpoint
    const response = await fetch(FOREMAN_GET_JOB_ENDPOINT);
    const job: MathJob = await response.json();
    // Do the "work"
    const result = job.body.values.reduce((acc, val) => acc + val, 0);
    // Submit the job result back to Foreman using the PUT endpoint
    await fetch(FOREMAN_PUT_JOB_ENDPOINT, {
        method: "PUT",
        headers: {
            "Content-Type": "application/json",
            "X-Foreman-Job-Status": "completed",
            "X-Foreman-Job-Progress": "1.0",
        },
        body: JSON.stringify({
            result,
        }),
    });
    // Keep process alive for 10 seconds
    let count = 0;
    while (true) {
        const sleep = new Promise((resolve) => setTimeout(resolve, 1000));
        await sleep;
        count++;
        if (count === 10) {
            break;
        }
    }
};

await main();
