// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import cl from 'classnames';
import { Link } from 'react-router-dom';

import { useAppSelector } from '../../hooks';
import { Toaster } from '../toaster';
import DappStatus from '_app/shared/dapp-status';
import { ErrorBoundary } from '_components/error-boundary';
import Logo from '_components/logo';
import { MenuButton, MenuContent } from '_components/menu';
import Navigation from '_components/navigation';

import type { ReactNode } from 'react';

import st from './PageMainLayout.module.scss';

export type PageMainLayoutProps = {
    children: ReactNode | ReactNode[];
    bottomNavEnabled?: boolean;
    topNavMenuEnabled?: boolean;
    dappStatusEnabled?: boolean;
    centerLogo?: boolean;
    className?: string;
};

export default function PageMainLayout({
    children,
    bottomNavEnabled = false,
    topNavMenuEnabled = false,
    dappStatusEnabled = false,
    centerLogo = false,
    className,
}: PageMainLayoutProps) {
    const networkName = useAppSelector(({ app: { apiEnv } }) => apiEnv);
    return (
        <div className={st.container}>
            <div className="fixed top-0 left-1/2 w-px h-36 bg-issue-dark z-50" />
            <div
                className={cl(st.header, {
                    [st.center]:
                        centerLogo && !topNavMenuEnabled && !dappStatusEnabled,
                })}
            >
                <div className={st.logoContainer}>
                    <Link to="/tokens" className="no-underline text-gray-90">
                        <Logo networkName={networkName} />
                    </Link>
                </div>
                {dappStatusEnabled ? (
                    <div className={st.dappStatusContainer}>
                        <DappStatus />
                    </div>
                ) : null}
                {topNavMenuEnabled ? (
                    <div className={st.menuContainer}>
                        <MenuButton className={st.menuButton} />
                    </div>
                ) : null}
            </div>
            <div className={st.content}>
                <main
                    className={cl(
                        st.main,
                        { [st.withNav]: bottomNavEnabled },
                        className
                    )}
                >
                    <ErrorBoundary>{children}</ErrorBoundary>
                </main>
                {bottomNavEnabled ? <Navigation /> : null}
                {topNavMenuEnabled ? <MenuContent /> : null}
                <Toaster bottomNavEnabled={bottomNavEnabled} />
            </div>
        </div>
    );
}
