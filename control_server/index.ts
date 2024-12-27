import { Application, Router } from "@oak/oak";

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
  return {
    id: crypto.randomUUID(),
    image: "vs-test-image:latest",
    command: [
      "/bin/sh",
      "-c",
      "echo 'I am a fake job simulating some work' && sleep 30",
    ],
    port: 80,
    body: {},
    callbackUrl: `http://localhost:${PORT}/job-response`,
    alwaysPull: false,
  };
};

const router = new Router();

router.get("/job", (ctx) => {
  console.log("headers:", ctx.request.headers);
  // TODO: Verify the agent id is valid and that it has permission to access this endpoint
  ctx.response.body = jobFactory();
});

router.put("/job/:jobId", (ctx) => {
  console.log("headers:", ctx.request.headers);
  // TODO: Verify the agent id is valid and that it has permission to access this endpoint
  // TODO: Update the job's status based on the response from the callback URL
  console.log(`Handling job response for job ${ctx.params.jobId}`);
  ctx.response.body = "ok";
});

const app = new Application();
app.use(router.routes());
app.use(router.allowedMethods());

console.log(`Server running at http://localhost:${PORT}`);
app.listen({ port: PORT });
