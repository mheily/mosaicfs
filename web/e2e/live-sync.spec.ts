import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Live sync', () => {
  test('node badges are visible on the dashboard from PouchDB sync', async ({ page }) => {
    await login(page);

    // The dashboard should show the "Node Health" section
    await expect(page.getByText('Node Health')).toBeVisible();

    // Wait for node data to load from PouchDB sync.
    // Either node badges appear or the "No nodes registered" message shows.
    const nodeBadge = page.locator('[class*="rounded-lg"][class*="border"][class*="bg-card"] >> text=/online|offline|degraded/i');
    const noNodes = page.getByText('No nodes registered');

    await expect(nodeBadge.first().or(noNodes)).toBeVisible({ timeout: 10000 });
  });
});
