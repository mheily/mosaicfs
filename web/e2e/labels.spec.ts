import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Labels', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('link', { name: 'Labels' }).click();
    await page.waitForURL('**/labels');
  });

  test('shows Assignments and Rules tabs', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Labels' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Assignments' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Rules' })).toBeVisible();
  });

  test('clicking Rules tab shows rules content with Add Rule button', async ({ page }) => {
    await page.getByRole('button', { name: 'Rules' }).click();

    // The "Add Rule" button should be visible
    await expect(page.getByRole('button', { name: 'Add Rule' })).toBeVisible();

    // Either a rules table or an empty-state message should appear
    const table = page.locator('table');
    const emptyMessage = page.getByText('No label rules');
    await expect(table.or(emptyMessage)).toBeVisible();
  });
});
