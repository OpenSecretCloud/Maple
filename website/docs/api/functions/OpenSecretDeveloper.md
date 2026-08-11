[**@opensecret/react**](../README.md)

***

[@opensecret/react](../README.md) / OpenSecretDeveloper

# Function: OpenSecretDeveloper()

> **OpenSecretDeveloper**(`props`): `Element`

Provider component for OpenSecret developer operations.
This provider is used for managing organizations, projects, and developer access.

## Parameters

### props

Configuration properties for the OpenSecret developer provider

#### apiUrl

`string`

URL of OpenSecret developer API

#### children

`ReactNode`

React child components to be wrapped by the provider

#### pcrConfig?

[`PcrConfig`](../type-aliases/PcrConfig.md) = `DEFAULT_PCR_CONFIG`

Optional PCR0 trust policy enforced before session establishment; its environment defaults to production

## Returns

`Element`

## Example

```tsx
<OpenSecretDeveloper
  apiUrl='https://developer.opensecret.cloud'
>
  <App />
</OpenSecretDeveloper>
```
