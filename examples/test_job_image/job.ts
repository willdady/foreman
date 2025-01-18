export interface Job {
    /**
     * Unique identifier for the job
     */
    id: string;

    /**
     * Docker image to use for the job
     */
    image: string;

    /**
     * Port to expose on the container
     */
    port: number;

    /**
     * Command to run in the container
     */
    command?: string[];

    /**
     * Body of the job, which can be any type
     */
    body: any;

    /**
     * Environment variables for the job
     */
    env?: { [key: string]: string };

    /**
     * Callback URL for the job
     */
    callbackUrl: string;

    /**
     * Whether to always pull the Docker image before creating a container
     */
    alwaysPull: boolean;
}
