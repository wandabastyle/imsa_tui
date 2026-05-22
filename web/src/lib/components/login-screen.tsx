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
    <section className="login-wrap">
      <div className="login-card">
        <h1>Live Timing Access</h1>
        <p>Enter the shared access code to open the timing dashboard.</p>
        <form
          className="login-form"
          onSubmit={(event): void => {
            event.preventDefault();
            onSubmit();
          }}
        >
          <input
            autoComplete="current-password"
            onChange={(event): void => {
              setLoginCode(event.target.value);
            }}
            placeholder="Access code"
            type="password"
            value={loginCode}
          />
          <button type="submit">Enter</button>
        </form>
        {loginError && <p className="login-error">{loginError}</p>}
      </div>
    </section>
  );
};
