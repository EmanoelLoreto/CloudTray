import { useHotkeys } from "react-hotkeys-hook";

const Button = ({
	onClick, 
	label,
	hotkey = '',
	icon,
	disabled = false
}: {
	onClick: () => void,
	label: string,
	hotkey?: string,
	icon?: React.ReactNode,
	disabled?: boolean
}) => {
	const getSymbolCombination = (hotkey: string) => {
		const modifierMap = {
			mod: navigator.platform.includes('Mac') ? '⌘' : 'Ctrl',
			shift: '⇧',
			alt: '⌥',
			ctrl: '⌃'
		}

		const keys = hotkey.split('+');
		return keys.map(key => modifierMap[key as keyof typeof modifierMap] || key.toUpperCase()).join(' ');
	}

	useHotkeys(
		hotkey,
		(event) => {
			event.preventDefault();
			onClick();
		},
		{
			enabled: Boolean(hotkey),
			preventDefault: true,
		}
	);

	const handleClick = () => {
		if (disabled) return;
		onClick();
	}

	return (
		<div className={`container-button ${disabled ? 'disabled' : ''}`} onClick={handleClick}>
			<div className="container-button-content">
				{icon && icon}
				<button className="button" disabled={disabled}>{label}</button>
			</div>
			<span className="hotkey">{getSymbolCombination(hotkey)}</span>
		</div>
	)
};

export default Button;