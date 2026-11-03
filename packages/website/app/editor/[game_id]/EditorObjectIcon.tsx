import type { IconType } from 'react-icons'
import { FaUserClock } from 'react-icons/fa6'
import { GiMeshBall } from 'react-icons/gi'
import { IoMdImages } from 'react-icons/io'
import { RiFolderFill, RiFolderLockFill, RiLockFill, RiVolumeUpFill } from 'react-icons/ri'
import { TbBone, TbCube, TbFileText, TbLetterT, TbMovie, TbPhoto, TbStopwatch, TbSun, TbUser } from 'react-icons/tb'
import { TfiTarget } from 'react-icons/tfi'

// Use React Icons equivalents of desktop symbols, with shared colors for related objects.
// The tree and composer share this mapping so a selected object keeps its visual identity.
const boneColor = '#e6d7b0'
const propertyColor = '#fb7185'
const iconOfKind: { [kind: string]: { Icon: IconType, color: string } } = {
    Ball: { Icon: TbCube, color: '#ef4444' },
    Brick: { Icon: TbCube, color: '#ef4444' },
    Cylinder: { Icon: TbCube, color: '#ef4444' },
    LocalGroup: { Icon: RiFolderFill, color: '#f5c842' },
    RigidGroup: { Icon: RiFolderLockFill, color: '#f5c842' },
    Mesh: { Icon: GiMeshBall, color: '#818cf8' },
    Label: { Icon: TbLetterT, color: '#38bdf8' },
    LabelImage: { Icon: TbPhoto, color: '#38bdf8' },
    Material: { Icon: IoMdImages, color: '#34d399' },
    Script: { Icon: TbFileText, color: '#a855f7' },
    Player: { Icon: TbUser, color: '#2dd4bf' },
    SoundPlayer: { Icon: RiVolumeUpFill, color: '#38bdf8' },
    Clock: { Icon: TbStopwatch, color: '#9ca3af' },
    Skeleton: { Icon: TbBone, color: boneColor },
    SkJoint: { Icon: TbBone, color: boneColor },
    SkAttachment: { Icon: RiLockFill, color: boneColor },
    SkAnimation: { Icon: TbMovie, color: boneColor },
    PropertyAnimation: { Icon: TbMovie, color: propertyColor },
    PropertyKeyframe: { Icon: TbMovie, color: propertyColor },
    SkKeyframe: { Icon: TbMovie, color: boneColor },
    SkKeyframePose: { Icon: TbMovie, color: boneColor },
    SkAnimator: { Icon: FaUserClock, color: boneColor },
    PropertyAnimator: { Icon: FaUserClock, color: '#7dd3fc' },
    Tween: { Icon: FaUserClock, color: '#7dd3fc' },
    Pivot: { Icon: TfiTarget, color: propertyColor },
    WorldLighting: { Icon: TbSun, color: '#facc15' },
}

export const EditorObjectIcon = ({ kind, className }: { kind: string, className: string }) => {
    const { Icon, color } = iconOfKind[kind]
    // Fill the same bone path for joints without changing its outline or stroke width.
    return <Icon aria-hidden="true" focusable="false" color={color} style={{ fill: kind === 'SkJoint' ? 'currentColor' : undefined }} className={`shrink-0 ${className}`} />
}
