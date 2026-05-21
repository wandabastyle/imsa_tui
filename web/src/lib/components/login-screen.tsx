// Login screen component
import type { JSX } from 'react';

interface LoginScreenProps {
  loginCode: string;
  loginError: string;
  setLoginCode: (value: string) => void;
  onSubmit: () => void;
}

export const LoginScreen = (props: LoginScreenProps): JSX.Element => {
  const { loginCode, loginError, setLoginCode, onSubmit } = props;

  return (
    <div className="login-container">
      <h1>IMSA Live Timing</h1>
      <input
        onChange={(event): void => {
          setLoginCode(event.target.value);
        }}
        placeholder="Enter access code"
        type="text"
        value={loginCode}
      />
      <button
        onClick={(): void => {
          onSubmit();
        }}
        type="button"
      >
        Login
      </button>
      {loginError && <div className="error">{loginError}</div>}
    </div>
  );
};
