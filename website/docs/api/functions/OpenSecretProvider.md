[**@opensecret/react**](../README.md)

***

[@opensecret/react](../README.md) / OpenSecretProvider

# Function: OpenSecretProvider()

> **OpenSecretProvider**(`props`): `Element`

Provider component for OpenSecret authentication and key-value storage.

## Parameters

### props

Configuration properties for the OpenSecret provider

#### apiUrl

`string`

URL of OpenSecret enclave backend

#### children

`ReactNode`

React child components to be wrapped by the provider

#### clientId

`string`

UUID identifying which project/tenant this instance belongs to

#### pcrConfig?

[`PcrConfig`](../type-aliases/PcrConfig.md) = `DEFAULT_PCR_CONFIG`

Optional PCR0 trust policy enforced before session establishment; its environment defaults to production

## Returns

`Element`

## Remarks

This provider manages:
- User authentication state
- Authentication methods (sign in, sign up, sign out)
- Key-value storage operations
- Project/tenant identification via clientId

## Example

```tsx
<OpenSecretProvider
  apiUrl='https://preview.opensecret.ai'
  clientId='550e8400-e29b-41d4-a716-446655440000'
>
  <App />
</OpenSecretProvider>
```
