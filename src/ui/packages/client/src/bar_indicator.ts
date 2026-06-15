import type { SvelteComponent } from 'svelte';
import { notificationStore, notificationCount } from './notification_store';

export class NotificationIndicator extends SvelteComponent {
  private count: number = 0;
  private unsubscribed = false;

  constructor(parent: HTMLElement) {
    super({ target: parent });
    
    // Subscribe to notification count changes.
    const unsubscribe = notificationCount.subscribe((n) => {
      this.count = n;
      this.$set({ count: n });
    });
    
    this.on('destroy', () => unsubscribe());
  }

  /** Toggle the widget between collapsed (toasts only) and expanded (with header). */
  toggle(): void {
    // Emit an event that the notification center view can respond to.
    const detail = JSON.stringify({ action: 'toggle' });
    window.webkit.messageHandlers.quantum.postMessage({ method: 'notifications.toggle', params: detail });
    
    this.$set({ expanded: !this.$$.ctx[0] }); // Toggle state in component
  }

  /** Dismiss a notification by ID. */
  dismiss(id: number): void {
    const detail = JSON.stringify({ id });
    window.webkit.messageHandlers.quantum.postMessage({ method: 'notifications.dismiss', params: detail });
  }

  /** Get the current count for badge display. */
  get badgeCount(): number {
    return this.count;
  }
}
