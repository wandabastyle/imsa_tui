// Loading and error screen components
import type { JSX } from 'react';

interface LoadingScreenProps {
  message?: string;
}

export const LoadingScreen = (props: LoadingScreenProps): JSX.Element => {
  const { message } = props;
  return <div className="loading">{message ?? 'Loading...'}</div>;
};

interface ErrorScreenProps {
  error: string;
}

export const ErrorScreen = (props: ErrorScreenProps): JSX.Element => {
  const { error } = props;
  return <div className="error">{error}</div>;
};
