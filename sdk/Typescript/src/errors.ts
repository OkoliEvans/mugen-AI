import { VeilErrorCode } from './types';

/**
 * All errors thrown by the Veil SDK are instances of VeilError.
 * Check the `code` field to handle specific failure modes.
 *
 * @example
 * try {
 *   await client.verifyInference(...)
 * } catch (err) {
 *   if (err instanceof VeilError && err.code === 'TIMEOUT') {
 *     // handle timeout
 *   }
 * }
 */
export class VeilError extends Error {
  public readonly code: VeilErrorCode;
  public readonly cause?: unknown;

  constructor(code: VeilErrorCode, message: string, cause?: unknown) {
    super(message);
    this.name = 'VeilError';
    this.code = code;
    this.cause = cause;

    // Restore prototype chain (required when extending built-in classes in TS)
    Object.setPrototypeOf(this, new.target.prototype);
  }
}