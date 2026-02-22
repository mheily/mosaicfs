import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Login flow', () => {
  test('redirects unauthenticated users to /login', async ({ page }) => {
    await page.goto('/');
    await expect(page).toHaveURL(/\/login/);
  });

  test('logs in with valid credentials and shows dashboard', async ({ page }) => {
    await login(page);

    // Should land on the dashboard
    await expect(page.getByRole('heading', { name: 'Dashboard' })).toBeVisible();

    // Node health strip section should be present
    await expect(page.getByText('Node Health')).toBeVisible();
  });

  test('logs out and redirects to /login', async ({ page }) => {
    await login(page);

    // Open the user dropdown in the top bar and click "Log out"
    await page.getByRole('button', { name: /admin/i }).click();
    await page.getByRole('menuitem', { name: 'Log out' }).click();

    await expect(page).toHaveURL(/\/login/);
    await expect(page.getByRole('heading', { name: 'MosaicFS' })).toBeVisible();
  });
});
