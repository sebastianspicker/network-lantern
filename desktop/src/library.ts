import { $ } from './dom';
import { errorMessage } from './model';

export type LibraryStatus = 'profiles-status' | 'reports-status';

// Each status line shows only the message of the most recent task that is still current.
const statusRevisions: Record<LibraryStatus, number> = { 'profiles-status': 0, 'reports-status': 0 };

export function beginTask(status: LibraryStatus, isCurrent: () => boolean) {
  const revision = ++statusRevisions[status];
  $(status).textContent = 'Working…';
  return {
    current: isCurrent,
    message(text: string) {
      if (isCurrent() && revision === statusRevisions[status]) $(status).textContent = text;
    },
    async run(operation: () => Promise<void>) {
      try {
        await operation();
      } catch (error) {
        this.message(errorMessage(error));
      }
    },
  };
}

export function invalidateStatus(status: LibraryStatus) {
  statusRevisions[status]++;
  $(status).textContent = '';
}
