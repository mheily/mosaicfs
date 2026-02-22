import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('VFS Editor', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('link', { name: 'Virtual FS' }).click();
    await page.waitForURL('**/vfs');
  });

  test('shows VFS directory tree', async ({ page }) => {
    await expect(page.getByText('VFS Tree')).toBeVisible();
  });

  test('clicking a directory shows mount editor section', async ({ page }) => {
    // Wait for the tree to load and click the first directory entry
    const treeButton = page.locator('aside button, [class*="w-64"] button').first();
    await treeButton.waitFor({ timeout: 10000 });
    await treeButton.click();

    // The right panel should show directory info with mount sources section
    await expect(page.getByText('Mount Sources')).toBeVisible({ timeout: 5000 });
  });
});
