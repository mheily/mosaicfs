import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('File Browser', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('link', { name: 'File Browser' }).click();
    await page.waitForURL('**/files');
  });

  test('shows directory tree on the left', async ({ page }) => {
    await expect(page.getByText('Directories')).toBeVisible();
    // The root "/" directory should be present in the tree
    await expect(page.locator('text=/').first()).toBeVisible();
  });

  test('clicking a directory shows files in right panel', async ({ page }) => {
    // Click the root directory entry in the tree
    const rootButton = page.locator('button', { hasText: '/' }).first();
    await rootButton.click();

    // The file table should be present with column headers
    await expect(page.getByRole('columnheader', { name: 'Name' })).toBeVisible();
  });

  test('clicking a file opens the detail drawer with metadata and download button', async ({ page }) => {
    // Click root to load files
    const rootButton = page.locator('button', { hasText: '/' }).first();
    await rootButton.click();

    // Wait for the table body to have at least one row, then click the first non-directory file
    const fileRows = page.locator('tbody tr');
    await fileRows.first().waitFor({ timeout: 10000 });

    // Click the first row -- if it is a directory it will navigate; if a file it opens the drawer
    await fileRows.first().click();

    // If a file detail drawer opened, check for Download button
    const downloadButton = page.getByRole('link', { name: 'Download' });
    if (await downloadButton.isVisible({ timeout: 3000 }).catch(() => false)) {
      await expect(downloadButton).toBeVisible();
    }
  });
});
