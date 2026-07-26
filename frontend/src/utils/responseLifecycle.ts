/**
 * Separates a deliberate client disconnect from a request failure. A disconnected response must
 * never enter retry/error recovery, because Maple's server continues processing it independently.
 */
export class ResponseLifecycleFence {
  private responseAborted = false;
  private responseInFlight = false;
  private unmounted = false;

  beginResponse() {
    if (this.unmounted || this.responseInFlight) return false;
    this.responseAborted = false;
    this.responseInFlight = true;
    return true;
  }

  finishResponse() {
    this.responseInFlight = false;
  }

  abortResponse() {
    this.responseAborted = true;
  }

  unmount() {
    this.unmounted = true;
    this.responseAborted = true;
    this.responseInFlight = false;
  }

  shouldIgnoreErrors() {
    return this.responseAborted || this.unmounted;
  }

  canUpdateState() {
    return !this.unmounted;
  }
}
