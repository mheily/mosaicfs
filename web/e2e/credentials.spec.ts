import { test, expect } from '@playwright/test';
import { login } from './helpers';

test.describe('Credentials', () => {
  test.beforeEach(async ({ page }) => {
    await login(page);
    await page.getByRole('link', { name: 'Settings' }).click();
    await page.waitForURL('**/settings');
  });

  test('shows Credentials tab', async ({ page }) => {
    await expect(page.getByRole('button', { name: 'Credentials' })).toBeVisible();
  });

  test('creates a new credential and sees it in the table', async ({ page }) => {
    // Click "Create Credential" button
    await page.getByRole('button', { name: 'Create Credential' }).click();

    // Fill in the credential name in the dialog
    const nameInput = page.getByPlaceholder('Credential name');
    await expect(nameInput).toBeVisible();
    const credName = `test-cred-${Date.now()}`;
    await nameInput.fill(credName);

    // Submit by clicking the Create button inside the dialog
    await page.locator('.fixed').getByRole('button', { name: 'Create' }).click();

    // A success banner with the secret key should appear
    await expect(page.getByText('Credential created')).toBeVisible({ timeout: 5000 });

    // The new credential should appear in the credentials table
    await expect(page.getByText(credName)).toBeVisible();
  });
});
