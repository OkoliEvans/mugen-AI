import { ElenxisErrorCode } from '../src/types';

/**
 * All errors thrown by the Elenxis SDK are instances of ElenxisError.
 * Check the `code` field to handle specific failure modes.
 *
 * @example
 * try {
 *   await client.verifyInference(...)
 * } catch (err) {
 *   if (err instanceof ElenxisError && err.code === 'TIMEOUT') {
 *     // handle timeout
 *   }
 * }
 */
export class ElenxisError extends Error {
  public readonly code: ElenxisErrorCode;
  public readonly cause?: unknown;

  constructor(code: ElenxisErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = 'ElenxisError';
    this.code = code;
    this.cause = cause;

    // Restore prototype chain (required when extending built-in classes in TS)
    Object.setPrototypeOf(this, new.target.prototype);
  }
}