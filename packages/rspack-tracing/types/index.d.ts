export function initOpenTelemetry(): Promise<void>;
export function shutdownOpenTelemetry(): Promise<void>;
export { trace, propagation, context } from "@opentelemetry/api";
export type { TraceAPI, ContextAPI, PropagationAPI } from "@opentelemetry/api";
export type { Tracer, Context } from "@opentelemetry/api";