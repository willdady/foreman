import { Application, Router } from "@oak/oak";

// If you change this, remember to update `core.url` in your `foreman.toml` file.
const PORT = 8888;

interface Job {
  id: string;
  image: string;
  command?: string[];
  port: number;
  body: any;
  env?: Record<string, string>;
  callbackUrl: string;
  alwaysPull: boolean;
}

const jobFactory = (): Job => {
  const id = crypto.randomUUID();
  return {
    id: crypto.randomUUID(),
    image: "foreman-test-job-image:latest",
    port: 80,
    body: {
      values: [
        5,
        10,
        15,
      ],
    },
    callbackUrl: `http://localhost:${PORT}/job/${id}`,
    alwaysPull: false,
  };
};

// Utility function for extracting labels from the x-foreman-labels header
const parseLabelsHeader = (
  encodedString: string,
): Record<string, string> => {
  const result: Record<string, string> = {};
  const pairs = encodedString.split(",");
  for (const pair of pairs) {
    const [encodedKey, encodedValue] = pair.split("=");
    const key = decodeURIComponent(encodedKey);
    const value = decodeURIComponent(encodedValue);
    result[key] = value;
  }
  return result;
};

// Returns a random integer between min (inclusive) and max (inclusive).
const getRandomInt = (min: number, max: number): number => {
  min = Math.ceil(min);
  max = Math.floor(max);
  return Math.floor(Math.random() * (max - min + 1)) + min;
};

const router = new Router();

router.get("/job", (ctx) => {
  console.log("--- GET /job ---");
  console.log("headers:", ctx.request.headers);

  // Parse labels from the x-foreman-labels header
  // In a concrete implementation, you might discrimate on these labels and only return jobs matching the labels.
  const labels = parseLabelsHeader(
    ctx.request.headers.get("x-foreman-labels") ?? "",
  );
  console.log("Got labels:", labels);

  // Extract token from the Authorization header.
  // In a concrete implementation, you would need to validate the token and reject the request if invalid!
  const token = (ctx.request.headers.get("authorization") ?? "").replace(
    "Bearer ",
    "",
  );
  console.log("Got token:", token);

  // Return an array of zero-or-more jobs, assigning them to the requesting foreman agent.
  // In a concrete implementation, you would mark each job as in-progress before returning them.
  // Never return a job that is already in-progress.
  const jobs = [];
  for (let i = 0; i < getRandomInt(0, 5); i++) {
    jobs.push(jobFactory());
  }
  ctx.response.body = jobs;
});

router.put("/job/:jobId", (ctx) => {
  console.log("--- PUT /job/:jobId ---");
  console.log(`Handling job response for job ${ctx.params.jobId}`);
  console.log("headers:", ctx.request.headers);

  const status = ctx.request.headers.get("x-foreman-job-status") as
    | "running"
    | "completed";
  console.log("Got job status:", status);

  const progress = parseFloat(
    ctx.request.headers.get("x-foreman-job-progress") ?? "0.0",
  );
  console.log("Got job progress:", progress);

  // In a concreate implementation, you would update the job's status based on the above
  // headers and optionally perform some action based on the request body.
  ctx.response.body = "ok";
});

const app = new Application();
app.use(router.routes());
app.use(router.allowedMethods());

console.log(`Server running at http://localhost:${PORT}`);
app.listen({ port: PORT });
