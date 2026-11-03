import { FaHatWizard } from 'react-icons/fa6'

// The hat fills its whole box where the other icons leave a margin, so it is drawn at 70% to look the same size beside them.
export const HatIcon = ({ className }: { className: string }) => <FaHatWizard className={`${className} scale-70`} aria-hidden="true" />
