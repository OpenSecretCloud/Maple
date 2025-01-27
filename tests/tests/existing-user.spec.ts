/// <reference types="node" />

import { config } from 'dotenv';
import { resolve } from 'path';
import { test, expect, Page } from '@playwright/test';

// Load environment variables from .env file
config({ path: resolve(__dirname, '../.env') });

const APP_URL = process.env.APP_URL;
const TESTER_EMAIL = process.env.TESTER_EMAIL;
const TESTER_PASSWORD = process.env.TESTER_PASSWORD;

test.beforeEach(async ({ page }) => {
  if (!APP_URL) {
    throw new Error('APP_URL environment variable is not set');
  }
  console.log('APP_URL:', APP_URL);

  if (!TESTER_EMAIL) {
    throw new Error('TESTER_EMAIL environment variable is not set');
  }
  console.log('TESTER_EMAIL:', TESTER_EMAIL);

  if (!TESTER_PASSWORD) {
    throw new Error('TESTER_PASSWORD environment variable is not set');
  }
  // Don't log the tester password

  await loginExistingUser(page);
  await expect(page.getByRole('heading', { name: 'History' })).toBeVisible({ timeout: 30000 });
});

async function loginExistingUser(page: Page) {
    await page.goto(APP_URL);
    await page.getByRole('button', { name: 'Log In' }).click();
    await page.getByPlaceholder('email@example.com').click();
    await page.getByPlaceholder('email@example.com').fill(TESTER_EMAIL);
    await page.getByPlaceholder('email@example.com').press('Tab');
    await page.getByLabel('Password').click();
    await page.getByLabel('Password').fill(TESTER_PASSWORD);
    await page.getByRole('button', { name: 'Log In' }).click();
  }

test('ask a question', async ({ page }) => {
    // Generate a random number between 1 and 100
    const randomCount = Math.floor(Math.random() * 100) + 1;
  
    // Ask a question
    await page.getByPlaceholder('Type your message here...').click();
    await page.getByPlaceholder('Type your message here...').fill(`Give me ${randomCount} random numbers between 0-100`);
    await page.getByRole('main').getByRole('button').click();
    
    // Check the answer
    await expect(page.getByRole('heading', { name: 'New Chat' })).toBeVisible({ timeout: 10000 });
    await expect(page.locator('#message-user-0').getByText(`Give me ${randomCount} random numbers`)).toBeVisible({ timeout: 10000 });
    await expect(page.getByText(`Here are ${randomCount} random numbers`)).toBeVisible({ timeout: 10000 });
  });