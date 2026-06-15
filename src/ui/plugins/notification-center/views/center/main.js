// Notification Center widget for Quantum.
// Shows stacked notification cards at top-right (collapsed) or with full header (expanded).

import { notificationStore, subscribeNotifications } from '@quantum/client';

function main() {
  const container = document.createElement('div');
  container.className = 'widget-container';
  container.style.cssText = `position: absolute; top: 24px; right: 24px; width: 360px; z-index: 999;`;

  // Header element.
  const header = document.createElement('div');
  header.className = 'widget-header';
  header.style.cssText = `background: #171720; border-bottom: 1px solid rgba(255,255,255,0.08); padding: 12px 16px;`;
  header.innerHTML = `<span style="font-weight: 600;">Notifications</span>`;

  // Cards container.
  const cardsContainer = document.createElement('div');
  cardsContainer.className = 'widget-list';
  cardsContainer.style.cssText = `overflow-y: auto; max-height: 440px;`;

  container.appendChild(header);
  container.appendChild(cardsContainer);
  document.body.appendChild(container);

  function renderCards(notifications) {
    // Clear existing.
    while (cardsContainer.firstChild) {
      cardsContainer.removeChild(cardsContainer.firstChild);
    }

    for (const notif of notifications) {
      const card = document.createElement('div');
      const urgencyClass = notif.urgency || 'normal';
      card.className = `notification-card ${urgencyClass}`;
      card.style.cssText = `margin-bottom: 6px; padding: 12px 14px; background: #171720; border-left: 3px solid; cursor: pointer; display: flex; align-items: flex-start; gap: 10px; box-shadow: 0 4px 16px rgba(0,0,0,0.4); animation: slideIn 0.3s ease forwards;`;
      card.innerHTML = `
        <div class="notif-icon">${notif.icon ? '' : '🔔'}</div>
        <div class="notif-body">
          <div class="notif-app">${notif.app_name}</div>
          <div class="notif-title">${notif.summary}</div>
          <div class="notif-summary">${notif.body}</div>
        </div>`;

      // Click to dismiss.
      card.addEventListener('click', () => {
        const payload = JSON.stringify({ action: 'dismiss', id: notif.id });
        if (typeof window.__quantum_notify === 'function') {
          window.__quantum_notify('notifications.event', payload);
        }
        // Animate out.
        card.style.animation = 'slideOut 0.25s ease forwards';
      });

      cardsContainer.appendChild(card);
    }
  }

  // Subscribe to notifications.
  subscribeNotifications((n) => renderCards(n));

  return { container, renderCards };
}

// Initialize on load.
if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', () => main());
} else {
  main();
}
