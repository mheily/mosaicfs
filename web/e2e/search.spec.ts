import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Search', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('link', { name: 'Search' }).click();
    await page.waitForURL('**/search');
  });

  test('shows search page with input', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Search' })).toBeVisible();
    await expect(page.getByPlaceholder(/Search files/)).toBeVisible();
  });

  test('typing a query shows results', async ({ page }) => {
    const searchInput = page.getByPlaceholder(/Search files/);
    await searchInput.fill('test');

    // Wait for results table to appear (debounce + network)
    await expect(page.locator('tbody tr').first()).toBeVisible({ timeout: 10000 });
  });

  test('clicking a result opens file detail drawer', async ({ page }) => {
    const searchInput = page.getByPlaceholder(/Search files/);
    await searchInput.fill('test');

    await page.locator('tbody tr').first().waitFor({ timeout: 10000 });
    await page.locator('tbody tr').first().click();

    // The file detail drawer/sheet should appear with a Download link
    await expect(page.getByRole('link', { name: 'Download' }).or(page.getByText('Download'))).toBeVisible({ timeout: 5000 });
  });
});
